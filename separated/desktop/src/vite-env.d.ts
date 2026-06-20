/// <reference types="vite/client" />

// Vite 默认的 client.d.ts 没有声明 .ico 资源，这里补上以便 TS 通过
declare module "*.ico" {
  const src: string;
  export default src;
}

// vite.config.ts 通过 define 注入，值来自 package.json
declare const __APP_VERSION__: string;
