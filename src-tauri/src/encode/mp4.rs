//! MP4 封装（§8.4 / §8.6）
//!
//! 使用 Media Foundation SinkWriter：输出媒体类型 H.264/AAC，输入类型 NV12/PCM，
//! SinkWriter 自动选择硬件或软件编码器。文件先写 `.tmp`，Finalize 后原子重命名。
//! 崩溃恢复的 `.tmp`/`.bak` 扫描为真实文件操作。

use crate::audio::aac::EncodedAudioSample;
use crate::encode::EncodedPacket;
use crate::error::{AppError, E_TIMEOUT};
use crate::models::RecoveryCandidate;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Media::MediaFoundation::{
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
    MFShutdown, MFStartup, IMFSample, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_Float,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_CHANNEL_MASK, MF_MT_AUDIO_VALID_BITS_PER_SAMPLE,
    MF_MT_AUDIO_FLOAT_SAMPLES_PER_SECOND, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_YUV_MATRIX, MF_VERSION, MFMediaType_Audio,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12,
};

/// MP4 SinkWriter 封装器。
pub struct Mp4Writer {
    final_path: PathBuf,
    tmp_path: PathBuf,
    writer: Option<IMFSinkWriter>,
    video_index: u32,
    audio_index: Option<u32>,
}

impl Mp4Writer {
    /// 创建 writer，输出先落到 `<name>.tmp`；随后 BeginWriting。
    pub fn new(
        final_path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        has_audio: bool,
    ) -> Result<Self> {
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建输出目录失败: {}", parent.display()))?;
        }
        // SinkWriter 按 URL 扩展名选择 MP4 容器，因此写入路径必须是 .mp4。
        // 用 `~` 前缀作为“未完成”标记，完成后原子重命名为最终文件。
        let mut tmp = final_path.to_path_buf();
        if let Some(name) = tmp.file_name().and_then(|n| n.to_str()) {
            tmp.set_file_name(format!("~{name}"));
        }

        unsafe {
            // 已初始化时返回 MF_E_MULTIPLESTARTUP，忽略即可。
            if let Err(e) = MFStartup(MF_VERSION, 0) {
                eprintln!("[MF] startup={:?}", e.code().0);
            } else {
                eprintln!("[MF] startup OK");
            }
        }

        let wide: Vec<u16> = tmp
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let writer = unsafe { MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, None) }
            .map_err(|e| anyhow!("创建 SinkWriter 失败: {e:?}"))?;

        // ---- 视频：输出 H.264，输入 NV12 ----
        let video_out = make_video_type(&MFVideoFormat_H264, width, height, fps, true)?;
        let video_index = unsafe { writer.AddStream(&video_out) }
            .map_err(|e| anyhow!("添加视频流失败: {e:?}"))?;
        let video_in = make_video_type(&MFVideoFormat_NV12, width, height, fps, false)?;
        unsafe { writer.SetInputMediaType(video_index, &video_in, None) }
            .map_err(|e| anyhow!("设置视频输入类型失败: {e:?}"))?;

        // ---- 音频：输出 AAC，输入 32-bit float PCM ----
        // 若系统缺少 AAC 编码器则优雅降级为无声视频，不阻断录制。
        let audio_index = if has_audio {
            match setup_audio_stream(&writer) {
                Ok(ai) => Some(ai),
                Err(err) => {
                    eprintln!("[MF] 音频流不可用，降级为无声: {err:#}");
                    None
                }
            }
        } else {
            None
        };

        unsafe { writer.BeginWriting() }.map_err(|e| anyhow!("BeginWriting 失败: {e:?}"))?;

        Ok(Self {
            final_path: final_path.to_path_buf(),
            tmp_path: tmp,
            writer: Some(writer),
            video_index,
            audio_index,
        })
    }

    /// 写入一帧 NV12 视频。
    pub fn write_video(&mut self, packet: &EncodedPacket) -> Result<()> {
        let writer = self.writer.as_ref().ok_or_else(|| anyhow!("writer 已释放"))?;
        let sample = make_sample(&packet.bytes, packet.timestamp_100ns, packet.duration_100ns)?;
        unsafe { writer.WriteSample(self.video_index, &sample) }
            .map_err(|e| anyhow!("写入视频帧失败: {e:?}"))
    }

    /// 写入一帧 16-bit PCM 音频。
    pub fn write_audio(&mut self, sample: &EncodedAudioSample) -> Result<()> {
        let index = match self.audio_index {
            Some(i) => i,
            None => return Ok(()), // 系统无 AAC 编码器时降级为无声
        };
        let writer = self.writer.as_ref().ok_or_else(|| anyhow!("writer 已释放"))?;
        let s = make_sample(&sample.bytes, sample.timestamp_100ns, sample.duration_100ns)?;
        unsafe { writer.WriteSample(index, &s) }
            .map_err(|e| anyhow!("写入音频帧失败: {e:?}"))
    }

    pub fn tmp_path(&self) -> &Path {
        &self.tmp_path
    }

    /// 周期备份 `.tmp` 已写部分到 `.bak`（§8.6）。
    pub fn backup(&self) -> Result<()> {
        if !self.tmp_path.exists() {
            return Ok(());
        }
        let mut bak = self.tmp_path.clone();
        bak.set_extension("bak");
        fs::copy(&self.tmp_path, &bak).context("备份临时 MP4 失败")?;
        Ok(())
    }

    /// Finalize 并原子重命名为最终路径。
    pub fn finalize(&mut self) -> Result<(String, u64)> {
        if let Some(writer) = self.writer.take() {
            unsafe { writer.Finalize() }.map_err(|e| anyhow!("Finalize 失败: {e:?}"))?;
            drop(writer);
        }
        let _ = unsafe { MFShutdown() };
        anyhow::ensure!(self.tmp_path.exists(), "没有可封装的临时文件");
        match fs::rename(&self.tmp_path, &self.final_path) {
            Ok(()) => {}
            Err(_) => {
                let _ = fs::remove_file(&self.final_path);
                fs::rename(&self.tmp_path, &self.final_path)
                    .with_context(|| format!("重命名失败: {}", self.final_path.display()))?;
            }
        }
        let size = fs::metadata(&self.final_path)?.len();
        Ok((self.final_path.to_string_lossy().into_owned(), size))
    }
}

/// 构造视频媒体类型。
fn make_video_type(
    subtype: &windows::core::GUID,
    width: u32,
    height: u32,
    fps: u32,
    compressed: bool,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let mt = unsafe { MFCreateMediaType() }.map_err(|e| anyhow!("MFCreateMediaType: {e:?}"))?;
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64)?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
        mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, 2)?;
        mt.SetUINT32(&MF_MT_YUV_MATRIX, 1)?; // MFVideoTransferMatrix_BT709
        if compressed {
            let bitrate = width as u64 * height as u64 * fps as u64 / 8;
            mt.SetUINT32(&MF_MT_AVG_BITRATE, bitrate.min(u32::MAX as u64) as u32)?;
        }
    }
    Ok(mt)
}

/// 在 SinkWriter 上配置 AAC 音频流；失败时由调用方降级为无声。
fn setup_audio_stream(writer: &IMFSinkWriter) -> Result<u32> {
    let out = make_audio_type(&MFAudioFormat_AAC, 192_000)?;
    let ai = unsafe { writer.AddStream(&out) }.map_err(|e| anyhow!("添加音频流失败: {e:?}"))?;
    let input = make_audio_type_float(48_000)?;
    unsafe { writer.SetInputMediaType(ai, &input, None) }
        .map_err(|e| anyhow!("设置音频输入类型失败: {e:?}"))?;
    Ok(ai)
}

/// 构造 32-bit 浮点 PCM 输入媒体类型（AAC 编码器要求）。
fn make_audio_type_float(rate: u32) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let mt = unsafe { MFCreateMediaType() }.map_err(|e| anyhow!("MFCreateMediaType: {e:?}"))?;
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
        mt.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
        mt.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32)?;
        mt.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 8)?;
        mt.SetUINT32(&MF_MT_AUDIO_CHANNEL_MASK, 0x3)?; // FRONT_LEFT | FRONT_RIGHT
        mt.SetUINT32(&MF_MT_AUDIO_VALID_BITS_PER_SAMPLE, 32)?;
        mt.SetUINT32(&MF_MT_AUDIO_FLOAT_SAMPLES_PER_SECOND, rate)?;
        mt.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 8)?;
    }
    Ok(mt)
}

/// 构造音频媒体类型。
fn make_audio_type(
    subtype: &windows::core::GUID,
    avg_bytes_per_sec: u32,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let mt = unsafe { MFCreateMediaType() }.map_err(|e| anyhow!("MFCreateMediaType: {e:?}"))?;
    unsafe {
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        mt.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        mt.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
        mt.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        mt.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?; // 2ch * 16bit
        mt.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, avg_bytes_per_sec)?;
        mt.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?; // raw AAC（MP4 要求）
    }
    Ok(mt)
}

/// 构造带时间戳的 IMFSample。
fn make_sample(bytes: &[u8], ts: i64, duration: i64) -> Result<IMFSample> {
    let sample = unsafe { MFCreateSample() }.map_err(|e| anyhow!("MFCreateSample: {e:?}"))?;
    let buffer = unsafe { MFCreateMemoryBuffer(bytes.len() as u32) }
        .map_err(|e| anyhow!("MFCreateMemoryBuffer: {e:?}"))?;
    unsafe {
        let mut ptr = std::ptr::null_mut::<u8>();
        let mut max = 0u32;
        let mut cur = 0u32;
        buffer.Lock(
            &mut ptr,
            Some(&mut max as *mut u32),
            Some(&mut cur as *mut u32),
        )?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
    }
    unsafe {
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(ts)?;
        sample.SetSampleDuration(duration)?;
    }
    Ok(sample)
}

/// 扫描输出目录中的 `.tmp` / `.bak`，生成恢复候选（§8.6 / AC-4）。
pub fn scan_recovery(output_dir: &Path) -> Result<Vec<RecoveryCandidate>> {
    let mut candidates = Vec::new();
    if !output_dir.exists() {
        return Ok(candidates);
    }
    let entries = fs::read_dir(output_dir).context("读取输出目录失败")?;
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_temp = ext == "tmp" || ext == "bak" || (ext == "mp4" && name.starts_with('~'));
        if !is_temp {
            continue;
        }
        let mut final_path = path.clone();
        if ext == "mp4" && name.starts_with('~') {
            final_path.set_file_name(&name[1..]); // 去掉 ~ 前缀
        } else {
            final_path.set_extension("mp4");
        }
        if seen.contains(&final_path) {
            continue;
        }
        seen.insert(final_path.clone());
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut bak = path.clone();
        bak.set_extension("bak");
        candidates.push(RecoveryCandidate {
            tmp_path: path.to_string_lossy().into_owned(),
            bak_path: if bak.exists() {
                Some(bak.to_string_lossy().into_owned())
            } else {
                None
            },
            size,
            final_path: final_path.to_string_lossy().into_owned(),
        });
    }
    Ok(candidates)
}

/// 恢复：优先用 `.bak`，否则用 `.tmp`，复制到 `.mp4`。
pub fn recover(candidate: &RecoveryCandidate) -> Result<String> {
    let source = PathBuf::from(
        candidate.bak_path.as_deref().unwrap_or(&candidate.tmp_path),
    );
    anyhow::ensure!(source.exists(), "恢复源文件不存在: {}", source.display());
    let final_path = PathBuf::from(&candidate.final_path);
    fs::copy(&source, &final_path)
        .with_context(|| format!("恢复失败: {}", final_path.display()))?;
    let _ = fs::remove_file(PathBuf::from(&candidate.tmp_path));
    if let Some(bak) = &candidate.bak_path {
        let _ = fs::remove_file(PathBuf::from(bak));
    }
    Ok(final_path.to_string_lossy().into_owned())
}

pub fn delete_temp(candidate: &RecoveryCandidate) -> Result<()> {
    let _ = fs::remove_file(PathBuf::from(&candidate.tmp_path));
    if let Some(bak) = &candidate.bak_path {
        let _ = fs::remove_file(PathBuf::from(bak));
    }
    Ok(())
}

pub fn no_recovery_error() -> AppError {
    AppError::new(E_TIMEOUT, "没有可恢复的录制文件", "请检查输出目录。", false)
}
