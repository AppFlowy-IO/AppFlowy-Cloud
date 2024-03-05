
Before running the test, AppFlowy Cloud need to run with nginx server by this command:

```shell
docker compose up -d
```


```shell

## Run test

> Before executing the test, you need to install the [Chrome Driver](https://chromedriver.chromium.org/downloads). If
> you are using a Mac, you can easily install it using Homebrew.
>
> ```shell
> brew install chromedriver
> ```

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