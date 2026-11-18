//! 文件命名与输出目录管理（§9.2 / §12）
//!
//! 按命名模板生成文件名（`{yyyyMMdd-HHmmss}` 等占位符），管理输出目录，
//! 以「临时文件 -> 原子替换」方式落盘，避免写坏文件。

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 展开路径中的 `~` 为 home 目录，并统一路径分隔符。
pub fn expand_output_dir(raw: &str) -> Result<PathBuf> {
    let normalized = raw.replace('/', std::path::MAIN_SEPARATOR_STR);
    if let Some(rest) = normalized
        .strip_prefix("~/")
        .or_else(|| normalized.strip_prefix("~\\"))
    {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("无法定位用户目录"))?;
        // rest 可能仍含 '/'，逐个组件 join 以保证平台原生分隔符。
        let mut p = home;
        for component in rest.split(std::path::MAIN_SEPARATOR) {
            if !component.is_empty() && component != "." {
                p.push(component);
            }
        }
        Ok(p)
    } else {
        Ok(PathBuf::from(normalized))
    }
}

/// 本地时间组件（无需 chrono 依赖）。
fn local_parts() -> (i32, u32, u32, u32, u32, u32) {
    // 使用系统本地时区偏移，近似到秒；仅用于文件命名。
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let offset = local_offset_seconds();
    let t = secs + offset;
    let days = t.div_euclid(86_400);
    let sod = t.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    (
        year,
        month,
        day,
        (sod / 3600) as u32,
        ((sod % 3600) / 60) as u32,
        (sod % 60) as u32,
    )
}

fn local_offset_seconds() -> i64 {
    #[cfg(windows)]
    {
        // NOTE: 使用 Win32 GetTimeZoneInformation 获取本地偏移；失败时按 UTC。
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz = TIME_ZONE_INFORMATION::default();
        let _ = unsafe { GetTimeZoneInformation(&mut tz) };
        tz.Bias as i64 * 60
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// 民用日期算法（days since 1970 -> Y/M/D）。
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (
        (y + if m <= 2 { 1 } else { 0 }) as i32,
        m as u32,
        d as u32,
    )
}

/// 渲染命名模板。支持 {yyyy}{MM}{dd}{HH}{mm}{ss} 与 {yyyyMMdd-HHmmss}。
pub fn format_filename(template: &str, ext: &str) -> String {
    let (y, mo, d, h, mi, s) = local_parts();
    let stamp = format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}");
    template
        .replace("{yyyyMMdd-HHmmss}", &stamp)
        .replace("{yyyy}", &format!("{y:04}"))
        .replace("{MM}", &format!("{mo:02}"))
        .replace("{dd}", &format!("{d:02}"))
        .replace("{HH}", &format!("{h:02}"))
        .replace("{mm}", &format!("{mi:02}"))
        .replace("{ss}", &format!("{s:02}"))
        + "."
        + ext
}

/// 保存图片字节到输出目录，返回最终路径。
pub fn save_image(bytes: &[u8], output_dir: &Path, naming: &str, ext: &str) -> Result<PathBuf> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("创建输出目录失败: {}", output_dir.display()))?;
    let name = format_filename(naming, ext);
    let mut path = output_dir.join(&name);

    // 同名则追加序号，避免覆盖。
    let mut counter = 1u32;
    while path.exists() {
        let stem = format!("{}-{counter}", name.trim_end_matches(&format!(".{ext}")));
        path = output_dir.join(format!("{stem}.{ext}"));
        counter += 1;
    }

    // 原子写入：写 .tmp 后重命名。
    let mut tmp = path.clone();
    tmp.set_extension(format!("{ext}.tmp"));
    fs::write(&tmp, bytes).with_context(|| format!("写入临时文件失败: {}", tmp.display()))?;
    match fs::rename(&tmp, &path) {
        Ok(()) => Ok(path),
        Err(_) => {
            let _ = fs::remove_file(&path);
            fs::rename(&tmp, &path)
                .with_context(|| format!("重命名失败: {}", path.display()))?;
            Ok(path)
        }
    }
}

/// 在文件管理器中打开输出目录。
pub fn reveal_dir(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .context("启动文件管理器失败")?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .context("启动文件管理器失败")?;
    }
    Ok(())
}

/// 在文件管理器中定位到指定文件并选中它。
pub fn reveal_file(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .context("启动文件管理器失败")?;
    }
    #[cfg(not(windows))]
    {
        if let Some(parent) = path.parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .context("启动文件管理器失败")?;
        }
    }
    Ok(())
}

/// 确保输出目录存在，返回展开后的路径。
pub fn ensure_output_dir(raw: &str) -> Result<PathBuf> {
    let path = expand_output_dir(raw)?;
    fs::create_dir_all(&path)
        .with_context(|| format!("创建输出目录失败: {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_687), (2023, 11, 26));
    }

    #[test]
    fn naming_template_renders_stamp() {
        let name = format_filename("{yyyyMMdd-HHmmss}", "png");
        assert!(name.ends_with(".png"));
        assert_eq!(name.len(), 15 + 4);
        assert!(name.chars().take(8).all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn expand_tilde() {
        let p = expand_output_dir("~/Pictures/SnapCap").unwrap();
        assert!(p.is_absolute());
        assert!(p.ends_with("Pictures") || p.ends_with("SnapCap"));
        // 路径分隔符必须统一为平台原生分隔符，不能混用 '/' 与 '\'。
        let s = p.to_string_lossy();
        #[cfg(windows)]
        {
            assert!(!s.contains('/'), "路径含前斜杠: {s}");
            assert!(s.contains('\\'), "路径缺少反斜杠: {s}");
        }
        // 保存与打开必须使用同一结果。
        assert_eq!(p, expand_output_dir("~/Pictures/SnapCap").unwrap());
    }

    #[test]
    fn expand_absolute_path_is_stable() {
        let raw = if cfg!(windows) { "C:\\Temp\\SnapCap" } else { "/tmp/snapcap" };
        assert_eq!(expand_output_dir(raw).unwrap(), PathBuf::from(raw));
    }
}
