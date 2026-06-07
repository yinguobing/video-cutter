# dnclip — 视频片段裁剪工具 (Rust + mpv 嵌入版)

**版本:** v0.1 (草案)  
**日期:** 2026-06-07  
**作者:** 小黑  
**状态:** 需求讨论

---

## 1. 产品概述

### 1.1 一句话概括

一个 Linux 桌面 GUI 应用，用 Rust 编写，嵌入 mpv 做视频播放，支持可视化时间线选择入/出点，一键导出 DNxHD/DNxHR 片段。

### 1.2 核心场景

> 我有一堆长视频素材（H.264 编码），DaVinci Resolve Free 不吃。我需要挑一小段导成 DNxHD，然后丢进 Resolve 剪。

### 1.3 为什么不在 Resolve 里直接做？

Resolve Free 的 Linux 版压根不能解码 H.264。

---

## 2. 目标与非目标

### 2.1 目标

| 特性 | 优先级 | 说明 |
|------|:------:|------|
| 加载视频文件并播放 | P0 | 流畅播放、seek、暂停 |
| 标记入点/出点 | P0 | 在时间线上可视化选择片段 |
| 转码导出 DNxHD/DNxHR | P0 | 调用 ffmpeg CLI 实现 |
| 显示当前时间/片段时长 | P1 | 精确到帧 |
| 逐帧/快进/快退 | P1 | 快捷键操作 |
| Savable 预设参数 | P2 | 记忆分辨率/码率/编码器偏好 |
| 批量处理 | P3 | 一个文件多个片段 |

### 2.2 非目标（明确不做）

- ❌ 不搞 timeline 多轨编辑
- ❌ 不做任何滤镜/特效
- ❌ 不导出非 DNxHD/DNxHR 的格式（但预留 future）
- ❌ 不跨平台（Linux only，后续可讨论）
- ❌ 不处理音频同步等高级问题

---

## 3. 技术架构

### 3.1 整体架构

```
┌─────────────────────────────────────────────────┐
│                  dnclip (Rust)                   │
│                                                   │
│  ┌─────────────┐  ┌──────────────┐              │
│  │  egui GUI    │  │  mpv IPC     │              │
│  │  · 文件加载   │  │  (JSON IPC)   │              │
│  │  · 时间线UI   │◀─┤  · 播放控制   │              │
│  │  · 参数面板   │  │  · 帧同步     │              │
│  │  · 导出按钮   │  │  · 状态轮询   │              │
│  └──────┬───────┘  └──────┬───────┘              │
│         │                 │                       │
│         ▼                 ▼                       │
│  ┌──────────────┐  ┌──────────────┐              │
│  │ ffmpeg CLI   │  │ mpv 进程     │              │
│  │ 编码至DNxHD   │  │ (实际渲染器)   │              │
│  └──────────────┘  └──────────────┘              │
└─────────────────────────────────────────────────┘
```

### 3.2 关键技术选型

| 组件 | 选择 | 理由 |
|------|------|------|
| GUI 框架 | egui (eframe) | Rust 原生，即时模式，开发效率高；可通过 egui_winit 嵌入外部窗口 |
| 视频播放 | mpv + JSON IPC | 成熟可靠，hwaccel 无痛，Linux 标配 |
| IPC | Unix socket / pipe | mpv 原生支持 `--input-ipc-server` |
| 转码 | `std::process::Command` | 直接调 ffmpeg CLI，零依赖，参数自由 |
| 时间轴渲染 | egui 原生 canvas | 简单的 drag rectangle + 时间刻度 |
| FFI 回读（可选帧精确时间） | `mpv` crate 的 observe_property | 免去手动 parse JSON IPC 的需要 |

### 3.3 mpv 进程管理

```
┌─────────────────────────────┐
│ Rust (dnclip)               │
│  · spawn: mpv --wid=<winid> │
│  · send: { "command":[...] }│
│  · poll: { "command":[...] }│
└─────────────────────────────┘
         │ IPC socket or stdin
         ▼
┌─────────────────────────────┐
│ mpv child process            │
│  · 解码 + 渲染到嵌入式窗口    │
│  · hwaccel (VAAPI/cuda)     │
│  · 音频直接出                │
│  · 接受 JSON IPC 命令        │
└─────────────────────────────┘
```

关键：
- 用 `--wid=<X11 window id>` 嵌入 mpv 窗口 → Rust 的 egui 窗口需要一个占位区域，想办法把 mpv 的渲染区域嵌入进去
- **备选方案**：如果不走嵌入窗口，也可以用 mpv 独立窗口，同步播放/暂停状态。简单但体验打折。

---

## 4. UI 设计

### 4.1 布局

```
┌──────────────────────────────────────────┐
│ [文件按钮]  当前文件: input.mp4           │
├──────────────────────────────────────────┤
│                                          │
│        视频预览区域 (mpv 窗口嵌入)          │
│        缩放适应 / 保持比例                │
│                                          │
├──────────────────────────────────────────┤
│  ┌──────┐         ┌────┐  ┌──────┐     │
│  │ ◀◀ │ ▸▸ 暂停  │ ■  │  │ ▶▶  │     │
│  └──────┘         └────┘  └──────┘     │
│  [00:01:23.456] ────●────────── [00:05:00.000] │
│  ∧入点             ∧当前位置           ∧出点    │
├──────────────────────────────────────────┤
│  入点: 00:01:23.456   出点: 00:02:30.000  │
│  时长: 00:01:06.544   帧率: 23.976        │
├──────────────────────────────────────────┤
│  [编码器: DNxHR HQ] [分辨率: 原始]        │
│  [输出路径: ...]  [ 导出片段 ]            │
└──────────────────────────────────────────┘
```

### 4.2 交互

| 操作 | 方式 |
|------|------|
| 播放/暂停 | 空格键 或 按钮 |
| 标记入点 | I 键 或 按钮 |
| 标记出点 | O 键 或 按钮 |
| 拖拽时间线 | 鼠标拖拽 + 实时更新预览 |
| 快进/快退 | 左右方向键（5秒）|
| 逐帧 | 方向键上/下（1帧）|
| 快捷键提示 | 悬浮窗或帮助面板 |

---

## 5. 数据流

### 5.1 主流程

```
用户打开视频文件
    │
    ▼
dnclip spawn mpv --wid=<embed_id> input.mp4
    │
    ▼
用户播放/暂停/seek → IPC 发命令给 mpv
    │
    ▼
用户标记 I/O 点 → Rust 记录帧时间戳
    │
    ▼
用户点击「导出」
    │
    ▼
构建 ffmpeg 参数:
  ffmpeg -ss <in_time> -i input.mp4 -to <duration> \
    -c:v dnxhd -profile:v dnxhr_hq \
    -pix_fmt yuv422p -c:a pcm_s16le output.mov
    │
    ▼
显示进度条（parse stderr 提取 time=）
    │
    ▼
输出文件 ready，可点击打开所在文件夹
```

### 5.2 关键数据模型

```rust
struct Project {
    source_path: PathBuf,
    video_info: VideoInfo,
    in_point: Option<f64>,  // 秒
    out_point: Option<f64>, // 秒
    export_params: ExportParams,
}

struct VideoInfo {
    width: u32,
    height: u32,
    fps: f64,
    duration: f64,
    codec: String,
}

struct ExportParams {
    profile: DnxProfile,  // HQ, SQ, LB, etc.
    output_path: Option<PathBuf>,
    // 保留原始分辨率？缩放？
    keep_original_resolution: bool,
}

enum DnxProfile {
    DnxHR_LB,     // 36 Mbps 1080p
    DnxHR_SQ,     // 60 Mbps 1080p
    DnxHR_HQ,     // 110 Mbps 1080p (default)
    DnxHR_HQX,    // 175 Mbps 1080p
    DnxHR_444,    // 非必要
}
```

---

## 6. 开发阶段

### Phase 1 — MVP（预计 2-3 天）

目标：能在嵌入的 mpv 窗口播放视频，标记 I/O 点，导出 DNxHD

- [ ] 项目初始化（Cargo.toml + egui bootstrap）
- [ ] mpv 子进程 spawn + IPC 通信
- [ ] 基本 UI 骨架（文件加载、播放控制、时间线滑块）
- [ ] 入/出点标记逻辑
- [ ] ffmpeg CLI 调用 + 导出按钮
- [ ] 工作原型验证（"我能用这个剪一个视频"）

### Phase 2 — 体验打磨（1-2 天）

- [ ] 快捷键绑定
- [ ] 时间线颜色（选中区域高亮）
- [ ] 导出进度反馈
- [ ] 错误处理（无效文件、ffmpeg 未安装、mpv 崩溃恢复）
- [ ] 配置文件（记住上次输出目录、编码器偏好）

### Phase 3 — 锦上添花（可选）

- [ ] 批量片段（同一个文件的多个 I/O 对）
- [ ] 缩略图时间线（thumbnails）
- [ ] 编码队列
- [ ] 暗色主题（egui 原生支持）

---

## 7. 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|:----:|:----:|----------|
| mpv 嵌入窗口在 egui 中不好实现 | 中 | 高 | 备选：独立 mpv 窗口 + 同步状态。不嵌入也能用 |
| 帧精确 seek 与 mpv IPC 延迟 | 中 | 中 | mpv 的 seek 精度足够（keyframe 附近可补帧） |
| 用户没有装 mpv/ffmpeg | 低 | 高 | 启动时检查 + 清晰提示安装命令 |
| 大文件加载/seek 卡顿 | 低 | 中 | mpv 自身优化好，seek 高效 |
| Rust GUI 开发调试慢 | 低 | 低 | egui 的编译-热重载已经很成熟 |
| 项目半途而废 | 中 | — | 先做 MVP，一两天就能用上，降低放弃风险 |

---

## 8. 开放问题

1. **窗口嵌入方案** — mpv 是否可以通过 `--wid` 嵌入到 egui 创建的窗口内？egui 默认是用 winit + glow/wgpu，这意味着我们需要：
   - 创建一个空的 native 窗口区域
   - 获取该区域的 X11 Window ID
   - 传给 mpv `--wid=<id>`
   - 这需要了解 egui/winit 底层 API

   **备选：** 如果嵌入做不到或者太麻烦，mpv 独立窗口 + 本窗口同步播放状态。用户体验差一点，但功能不受影响。

2. **IPC 协议选型** — 用 mpv 的 JSON IPC (Unix socket) 还是 mpv crates？

3. **资源打包** — 是纯源代码项目（靠用户自己装 mpv/ffmpeg）还是考虑 flatpak/AppImage？

4. **命名** — dnclip？dnxtractor？还是别的？

---

## 9. 快速开始（用户视角）

```bash
# 依赖
sudo apt install mpv ffmpeg

# 运行
dnclip input.mp4

# 快捷键
Space  — 播放/暂停
I      — 标记入点
O      — 标记出点
←/→    — 快退/快进 5s
↑/↓    — 逐帧前进/后退
Enter  — 导出选中片段
Ctrl+O — 打开新文件
```

---

## 10. Appendix: 相关作品参考

- **LosslessCut** — Electron 写的，视觉操作一流，但不转码
- **LosslessCut Rust** → 好像没有
- **mpv 作为裁剪前端** — mpv 本身可以用 `{ "command": ["set", "file-filter", "..."] }` 做裁剪但不够直观
- **ffmpeg + pyQT 工具** — 很多实现，但都是 Python

---

*这个是 PRD v0.1，欢迎讨论再细化。*
