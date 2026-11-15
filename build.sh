#!/bin/sh
# Build the plugin package: tests, wasm32 core module, component, zip.
set -e
cd "$(dirname "$0")/plugin"
~/.cargo/bin/cargo test --quiet
RUSTFLAGS="--remap-path-prefix=$HOME=~" ~/.cargo/bin/cargo build --release --target wasm32-unknown-unknown
~/.cargo/bin/wasm-tools component new target/wasm32-unknown-unknown/release/opengrid.wasm -o ../package/opengrid/plugin.wasm
cd ../package
rm -f opengrid-*.zip
VERSION=$(python3 -c "import json;print(json.load(open('opengrid/plugin.json'))['version'])")
python3 -c "import zipfile; z=zipfile.ZipFile('opengrid-$VERSION.zip','w',zipfile.ZIP_DEFLATED); [z.write('opengrid/'+f) for f in ('plugin.json','plugin.wasm')]"
echo "package/opengrid-$VERSION.zip"
