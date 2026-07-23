# Node Speedtest Desktop

Tauri 2 + React + TypeScript 桌面端。

## 开发

```bash
npm install
pwsh -File scripts/sync-engine.ps1
npm run tauri dev
```

## 打包

```bash
npm run tauri build
```

安装包输出：`src-tauri/target/release/bundle/nsis/`
