<div align="center">

  <h1><code>Client API WASM</code></h1>

  <strong>Client-API to WebAssembly Compiler</strong>

</div>

## 🚴 Usage

### 🐑 Prepare

```bash
# Clone the repository (if you haven't already)
git clone https://github.com/AppFlowy-IO/AppFlowy-Cloud.git

# Navigate to the client-for-wasm directory
cd libs/client-api-wasm

# Install the dependencies (if you haven't already)
cargo install wasm-pack
```

### 🛠️ Build with `wasm-pack build`

```
wasm-pack build
```

### 🔬 Test in Headless Browsers with `wasm-pack test`

```bash
# Ensure you have geckodriver installed
wasm-pack test --headless --firefox

# or
# Ensure you have chromedriver installed
# https://googlechromelabs.github.io/chrome-for-testing/
# Example (Linux):
# 1. wget https://storage.googleapis.com/chrome-for-testing-public/123.0.6312.86/linux64/chromedriver-linux64.zip
# 2. unzip chromedriver-linux64.zip
# 3. sudo mv chromedriver /usr/local/bin
# 4. chromedriver -v
# If you see the version, then you have successfully installed chromedriver
# Note: the version of chromedriver should match the version of chrome installed on your system
wasm-pack test --headless --chrome
```

### 🎁 Publish to NPM with ~~`wasm-pack publish`~~

##### Don't publish in local development, only publish in github actions

```
wasm-pack publish
```

### 📦 Use your package as a dependency

```
npm install --save @appflowy/client-api-for-wasm
```

### 📝 How to use the package in development?

See the [README.md](https://github.com/AppFlowy-IO/AppFlowy/tree/main/frontend/appflowy_web_app/README.md) in the AppFlowy Repository.
