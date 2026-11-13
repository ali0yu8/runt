import { defineConfig } from "vite";

// Relative paths, so the build runs from a sub-folder or any static host.
export default defineConfig({ base: "./", build: { target: "es2022" } });
