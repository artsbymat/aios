# AIOS Plugin Registry (`$0/month` GitHub-Native Store)

This directory mirrors the exact layout of a standalone `aios-plugins-registry` repository so it can be split into a dedicated community repo at any time without changing a single schema field or client parser.

## Structure

```text
registry/
  plugins/
    com.aios.calculator.toml
    <your-plugin-id>.toml
```

`index.json` is never committed manually. GitHub Actions automatically compiles `registry/plugins/*.toml` and publishes it to GitHub Pages at `https://artsbymat.github.io/aios/index.json` on every merge to `main`.

## How to Publish a Plugin (100% Free)

1. Scaffold and build your plugin locally:
   ```bash
   aios plugin new my-tool --id com.yourname.my-tool --author "Your Name"
   cd my-tool
   aios plugin build .
   aios plugin dev .
   ```
2. Pack and publish a GitHub Release on your own public repository (the scaffolded `.github/workflows/release.yml` automates this on every `v*` git tag):
   ```bash
   aios plugin pack .
   git tag v0.1.0 && git push --tags
   ```
3. Open a Pull Request adding `registry/plugins/<your-plugin-id>.toml` using the snippet printed by `aios plugin pack .`.
4. GitHub Actions automatically verifies the TOML schema, downloads your `.aios-plugin` release asset, and checks the exact **SHA-256** digest.
5. Once merged, `index.json` is rebuilt and published automatically to GitHub Pages / jsDelivr CDN so every AIOS user can install it in 1 click via **Plugin Store (`aios://store`)** or `aios plugin install <your-plugin-id>`.

## Installing Without Registry Listing

Users can also install directly from any public GitHub Release or local archive:

```bash
aios plugin install github:owner/repo
aios plugin install ./com.yourname.my-tool-v0.1.0.aios-plugin
```
