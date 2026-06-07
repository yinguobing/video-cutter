# dnclip — Video Cutter

**一个 Linux 桌面 GUI 视频裁剪工具。** 嵌入 mpv 做播放引擎，支持可视化时间线选择入/出点，一键导出 DNxHD/DNxHR 片段给 DaVinci Resolve 使用。

## 解决了什么问题

DaVinci Resolve Free 在 Linux 上不支持 H.264/H.265 解码。你需要先把素材转成 DNxHD 才能导入。

但原始素材动辄几十分钟，你需要的只是其中一小段。这个工具让你：

1. 打开视频 → mpv 嵌入窗口直接播放
2. 快捷键标记 I/O 点
3. 一键导出 DNxHD（或攒多个片段批量导出）

## 快速开始

```bash
# 依赖
sudo apt install mpv ffmpeg

# 运行
cargo run --release -- /path/to/video.mp4

# 或者编译后直接跑
cargo build --release
./target/release/dnclip video.mp4
```

## 快捷键

| 键 | 操作 |
|-----|------|
| `Space` | 播放 / 暂停 |
| `I` | 标记入点 |
| `O` | 标记出点 |
| `←` / `→` | 快退 / 快进 5 秒 |
| `↑` / `↓` | 逐帧前进 / 后退 |
| `Enter` | 导出所有片段 |
| `Ctrl+O` | 打开文件（对话框） |
| `Ctrl+D` | 调试模式（显示 widget 信息） |
| `H` | 快捷帮助 |

## 效果

`Ctrl+O` 或拖拽文件打开视频 → mpv 窗口嵌入到 egui 预览区 → 播放、seek、逐帧 → 按 I/O 标记 → 右侧面板管理多个片段 → 选 DNxHR 配置 → 点击 Export → 输出 `.mov` 文件，直接丢进 Resolve。

## 技术架构

```
┌──────────────────────────────────────────────────┐
│                  dnclip (Rust)                    │
│                                                   │
│  ┌──────────────┐   ┌──────────────────────┐     │
│  │  egui GUI     │   │  mpv (JSON IPC)      │     │
│  │  · 时间线     │◀──┤  · 解码 + 渲染       │     │
│  │  · I/O标记    │   │  · X11 child window  │     │
│  │  · 多片段     │   │  · hwaccel           │     │
│  └──────┬───────┘   └──────────────────────┘     │
│         │                                         │
│         ▼                                         │
│  ┌────────────────┐                               │
│  │ ffmpeg CLI      │                               │
│  │ DNxHD/DNxHR 编码 │                               │
│  └────────────────┘                               │
└──────────────────────────────────────────────────┘
```

### 关键组件

| 组件 | 方案 | 说明 |
|------|------|------|
| **GUI** | egui (eframe) | Rust 即时模式 GUI |
| **播放引擎** | mpv + JSON IPC | 通过 Unix socket 发送 JSON 命令控制 mpv |
| **窗口嵌入** | X11 child window (`x11-dl`) | 在 egui 预览区创建子窗口，mpv 往里面渲染 |
| **转码** | ffmpeg CLI | `std::process::Command` 调用，零依赖 |
| **文件选择** | egui-file-dialog | 原生风格文件选择器 + 拖拽支持 |

### 项目结构

```
src/
├── main.rs      # 入口 + eframe 初始化
├── app.rs       # egui App 实现（UI + 逻辑）
├── embed.rs     # X11 子窗口管理
├── player.rs    # mpv 进程管理 + JSON IPC
├── export.rs    # ffmpeg 命令行封装
└── types.rs     # 数据模型
```

## 编码配置

| Profile | 码率 (1080p) | ffmpeg profile |
|---------|:------------:|----------------|
| DNxHR LB | 36 Mbps | `dnxhr_lb` |
| DNxHR SQ | 60 Mbps | `dnxhr_sq` |
| **DNxHR HQ** (默认) | **110 Mbps** | `dnxhr_hq` |
| DNxHR HQX | 175 Mbps | `dnxhr_hqx` |

## 依赖

- **Rust** (edition 2021)
- **mpv** — 播放引擎
- **ffmpeg** — 编码引擎
- **X11** — 窗口嵌入（libx11-dev）

## License

MIT
