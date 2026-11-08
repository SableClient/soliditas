export interface Env {
  HOSTNAME?: string;
  SERVERNAME?: string;
  PORT?: number;
}

declare module '*.wasm' {
  const module: WebAssembly.Module;
  export default module;
}
