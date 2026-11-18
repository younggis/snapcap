# 合成测试帧生成器

对应设计文档 `docs/DESIGN.md` §16.1「编码管线」测试。

## 目的

编码管线测试需要**确定性的输入帧**：将合成的 D3D 纹理（纯色 / 棋盘格 / 渐变）送入编码 → MP4 封装管线，再用 `ffprobe`（仅开发 / CI）校验分辨率、帧率、时长与音轨。

使用合成帧而非真实屏幕内容的好处：

- **可复现**：像素值完全确定，便于断言与回归对比。
- **无隐私**：不涉及真实屏幕内容。
- **可度量**：棋盘格便于检测缩放 / 裁剪错误，纯色便于检测色偏，渐变便于检测像素格式（BGRA→NV12）转换错误。

生成器的输出为 **24-bit BMP 文件**（无压缩，便于测试代码直接读取像素）。在实际 D3D 测试中，这些图案会被加载为纹理；本脚本只负责生成图案数据。

## 用法

```powershell
# 生成 12 帧 1920x1080，输出到 ./frames（循环 纯色 → 棋盘格 → 渐变）
python tests/fixture/generate_frames.py --width 1920 --height 1080 --count 12 --out frames

# 只生成棋盘格，指定单帧尺寸
python tests/fixture/generate_frames.py --width 1280 --height 720 --count 4 --out frames --pattern checkerboard
```

参数：`--width`（宽）、`--height`（高）、`--count`（帧数）、`--out`（输出目录）。可选 `--pattern`（`solid` / `checkerboard` / `gradient` / `all`，默认 `all`）。

仅使用 Python 标准库（`struct` / `argparse` / `os`），无外部依赖。详见脚本内 docstring。
