https://crates.io/crates/wacker Like Docker, but for WASM. Interesting !!
cargo install wacker-daemon
cargo install wacker-cli

```
(base) ➜  clickplanet-client git:(feat-webassembly) ✗ wacker serve pkg/clickplanet_webapp_bg.wasm --addr 0.0.0.0:8080
Error: transport error

Caused by:
    0: No such file or directory (os error 2)
    1: No such file or directory (os error 2)
```

https://crates.io/crates/wepl have a look to exports of WA package
cargo install wpel

```
(base) ➜  clickplanet-client git:(feat-webassembly) ✗ wepl pkg/clickplanet_webapp_bg.wasm
Error: failed to parse WebAssembly module

Caused by:
  attempted to parse a wasm module with a component parser
```

https://crates.io/crates/world-map-gen random game worlds
https://crates.io/crates/wrpc looks like gRPC and based on WIT https://component-model.bytecodealliance.org/design/wit.html

https://crates.io/crates/wasm-bindgen-rayon rayon for wasm


Promising:
https://crates.io/crates/wasm-react Interesting, works with React. To be checked
https://crates.io/crates/frender To be checked, looks nice
https://dioxuslabs.com/ really really interesting, to be tested (used by AIRBUS and ESA)
https://leptos.dev/ 
https://github.com/Eliot00/dioxus-react-example/blob/master/react-components/src/App.js
https://bevyengine.org/ read about, promising
https://crates.io/crates/dip less interesting than dioxus, but worth a try maybe(not published since two years)


https://crates.io/crates/observable-react To be tested
https://crates.io/crates/react-sys Not ready for production
https://crates.io/crates/cargo-component read about
https://rustwasm.github.io/wasm-bindgen/examples/webgl.html
https://github.com/allsey87/threejs-sys

=> ADR: It will use Dioxus, THREE manual bindings, and wgpu crate
