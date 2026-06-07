#!/bin/bash
set -xe

apk add gcc g++ build-base linux-headers cmake make autoconf automake libtool git
apk add libpng-dev libpng-static openssl-dev openssl-libs-static curl-dev curl-static nghttp2-static freetype-dev freetype-static zlib-dev zlib-static rapidjson-dev libevent-dev libevent-static bzip2-static pcre2-dev pcre2-static brotli-static
# harfbuzz (+ its static deps) is required since the colour-emoji renderer was added.
# gettext-static provides libintl that glib-static needs on musl.
apk add harfbuzz-dev harfbuzz-static graphite2-static glib-static gettext-static

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
# freetype<->harfbuzz have a circular dependency when freetype is built with
# harfbuzz support, so wrap them in a link group. glib/graphite2 back harfbuzz.
g++ -o base/stairspeedtest CMakeFiles/stairspeedtest.dir/src/*.o -static \
    -lpcre2-8 -levent -lyaml-cpp -lPNGwriter -lpng \
    -Wl,--start-group -lfreetype -lharfbuzz -Wl,--end-group \
    -lgraphite2 -lglib-2.0 -lintl \
    -lcurl -lnghttp2 -lssl -lcrypto -lz -lbz2 -lbrotlidec -lbrotlicommon \
    -ldl -lpthread -O3 -s

chmod +rx base/stairspeedtest base/*.sh
chmod +r base/*
