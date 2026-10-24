# Vocal-inputer

macOS 系统级语音输入（应用名 VoiceInput）。在任何应用里按住快捷键说话，松开后整理好的文字直接出现在光标处。本地模型离线可用、边说边出字，云端模型在后台提升准确度，两者自动融合。

## 需求

macOS 桌面端语音输入：在任何应用中按住全局快捷键说话，松开后把整理好的书面文字插入当前输入框。本地模型边说边出字（离线可用），云端模型在后台提高准确度，结束后合并结果，不让用户等待。

## 技术选型

- 第二版定稿与实测数据：[docs/tech-selection-v2.md](docs/tech-selection-v2.md)
- 第一版候选方案对比：[docs/tech-selection.md](docs/tech-selection.md)

要点：Tauri 2 + Rust；本地 sherpa-onnx（流式 Zipformer 预览 + SenseVoice 精识别 + Silero VAD）；云端 OpenRouter（默认 Voxtral Mini Transcribe 3），VAD 分块边说边上传；规则整理 + 按需 LLM 整理（gpt-oss-20b）；CGEventTap 快捷键；NSPanel 浮窗；剪贴板粘贴并恢复；钥匙串存密钥；历史只存文字。

## 目录结构

```
docs/          技术选型文档
src/           React 界面（主窗口、引导、悬浮指示器）
src-tauri/     Rust 后端（快捷键、录音、识别、整理、插入、存储）
index.html     主窗口入口
overlay.html   悬浮指示器入口
```

## 开发与构建

```bash
npm install
npm run tauri dev                       # 开发模式
npm run tauri build -- --bundles app    # 构建并签名 .app
cd src-tauri && cargo test --lib        # 文本处理单元测试
cargo run --features bench --bin vi_bench -- 音频.wav   # 模拟实时说话，测量延迟
```

OpenRouter API Key 只保存在 macOS 钥匙串（服务名 `voice-input-desktop`），不写入任何项目文件；也可以在 App 的设置里填写或更换。本地模型首次使用时在 App 内下载（约 440 MB），国内网络自动使用 hf-mirror.com 镜像。

从源码构建时，如果本机没有名为 “VoiceInput Dev” 的签名证书，可用 `APPLE_SIGNING_IDENTITY="-" npm run tauri build -- --bundles app` 改为本地临时签名，或把 `src-tauri/tauri.conf.json` 中的 `signingIdentity` 换成你自己的证书。

作者本机开发时使用自签名证书（独立钥匙串 `voiceinput-dev`），每次重新编译后首次读取 Key 时 macOS 会弹出一次钥匙串授权，点“始终允许”即可；麦克风与辅助功能授权在重新编译后保持有效。

## 进度

- [x] 需求确认
- [x] 技术选型（第二版定稿）
- [ ] 实现（M1：核心链路与界面已完成，真机验收中）
- [ ] 演示

## 许可证

本项目以 [MIT License](LICENSE) 开源。

本地识别模型（sherpa-onnx 流式 Zipformer、SenseVoice、Silero VAD）不包含在本仓库中，由 App 在首次使用时从上游下载，使用时请遵守各模型上游仓库的许可证。云端识别通过 OpenRouter 调用，需要使用你自己的 API Key，并遵守相应服务条款。
