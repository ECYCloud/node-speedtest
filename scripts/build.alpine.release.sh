#!/bin/bash
set -xe

apk add gcc g++ build-base linux-headers cmake make autoconf automake libtool git
apk add libpng-dev libpng-static openssl-dev openssl-libs-static curl-dev curl-static nghttp2-static freetype-dev freetype-static zlib-dev zlib-static rapidjson-dev libevent-dev libevent-static bzip2-static pcre2-dev pcre2-static brotli-static
# harfbuzz (+ its static deps) is required since the colour-emoji renderer was added.
# gettext-static provides libintl that glib-static needs on musl.
apk add harfbuzz-dev harfbuzz-static graphite2-static glib-static gettext-static
# Static curl drags in psl + c-ares + idn2 + unistring + zstd transitively.
apk add c-ares-static libpsl-static libidn2-static libunistring-static zstd-static

git clone https://github.com/jbeder/yaml-cpp --depth=1
cd yaml-cpp
cmake -DYAML_CPP_BUILD_TESTS=OFF -DYAML_CPP_BUILD_TOOLS=OFF -DCMAKE_POLICY_VERSION_MINIMUM=3.5 .
make install -j4
cd ..

git clone https://github.com/pngwriter/pngwriter --depth=1
cd pngwriter
cmake -DCMAKE_POLICY_VERSION_MINIMUM=3.5 .
make install -j4
cd ..

cmake .
make -j4
rm stairspeedtest
# Wrap every static lib in a single link group so circular/transitive deps
# (freetype<->harfbuzz, curl->psl/c-ares/idn2/zstd, glib->intl) resolve
# regardless of order. -lstdc++ is needed for harfbuzz/graphite2 C++ symbols.
g++ -o base/stairspeedtest CMakeFiles/stairspeedtest.dir/src/*.o -static \
    -Wl,--start-group \
    -lpcre2-8 -levent -lyaml-cpp -lPNGwriter -lpng \
    -lfreetype -lharfbuzz -lgraphite2 -lglib-2.0 -lintl \
    -lcurl -lnghttp2 -lssl -lcrypto -lz -lbz2 -lbrotlidec -lbrotlicommon \
    -lcares -lpsl -lidn2 -lunistring -lzstd -lstdc++ \
    -Wl,--end-group \
    -ldl -lpthread -O3 -s

chmod +rx base/stairspeedtest base/*.sh
chmod +r base/*
