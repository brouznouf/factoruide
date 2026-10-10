# Factoruide desktop app

Tauri app: React + TypeScript frontend (Vite) in `src/`, Rust backend in `src-tauri/` (commands
calling the `fg-route` planner, embedded databases, overrides and addon).

```sh
npm install
npm run tauri dev                      # development, with hot reload
npm run build                          # type check (tsc) and frontend build
npx oxlint --deny-warnings             # lint (.oxlintrc.json)
npm run tauri build -- --no-bundle     # release executable
```

The quest databases are versioned in `../data/`.
