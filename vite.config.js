import { defineConfig } from "vite";

// Tauri 的 devUrl 固定 1420，strictPort 保证端口被占时立刻失败
// （换端口会让 tauri dev 白等一个不存在的页面）。
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "esnext",
    // 两个窗口 = 两个 HTML 入口。少了这一行，`vite build` 只会产出
    // index.html，打包后的 app 里聊天窗口会 404（dev 模式看不出来）。
    rollupOptions: {
      input: {
        main: "index.html",
        chat: "chat.html",
        history: "history.html",
      },
    },
  },
});
