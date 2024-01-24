
## Run test

before running the test, it requires to install the [chrome driver](https://chromedriver.chromium.org/downloads).
for mac user, you can install it by brew.

```shell
brew install chromedriver
```

then run the test


```shell
wasm-pack test --headless  --chrome
```

## Testing in browser

```shell
wasm-pack test --chrome
```

Ref:
[wasm-bindgen-test](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/browsers.html)