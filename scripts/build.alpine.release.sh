#!/bin/bash
set -xe

apk add gcc g++ build-base linux-headers cmake make autoconf automake libtool git
apk add libpng-dev libpng-static openssl-dev openssl-libs-static zlib-dev zlib-static rapidjson-dev libevent-dev libevent-static bzip2-static pcre2-dev pcre2-static brotli-static freetype-dev freetype-static
# harfbuzz (+ its static deps) is required since the colour-emoji renderer was added.
# gettext-static provides libintl that glib-static needs on musl.
apk add harfbuzz-dev harfbuzz-static graphite2-static glib-static gettext-static

# Build a minimal static curl from source (HTTP/HTTPS only). Alpine's curl-static
# is built with psl/c-ares/idn2/brotli/nghttp2, dragging in a long chain of extra
# static .a files that aren't all packaged. A trimmed openssl-only curl avoids
# the whole transitive-dependency mess and is all this tool needs.
git clone https://github.com/curl/curl --depth=1
cd curl
cmake -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_CURL_EXE=OFF \
      -DHTTP_ONLY=ON -DCURL_USE_OPENSSL=ON \
      -DCURL_USE_LIBPSL=OFF -DCURL_USE_LIBSSH2=OFF -DUSE_NGHTTP2=OFF \
      -DCURL_BROTLI=OFF -DCURL_ZSTD=OFF -DENABLE_ARES=OFF \
      -DCURL_DISABLE_LDAP=ON -DCURL_DISABLE_LDAPS=ON \
      -DCMAKE_INSTALL_PREFIX=/usr/local -DCMAKE_POLICY_VERSION_MINIMUM=3.5 .
make install -j4
cd ..

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
# (freetype<->harfbuzz, glib->intl) resolve regardless of order. Our minimal
# curl only needs openssl + zlib. -lstdc++ backs harfbuzz/graphite2 C++ symbols.
g++ -o base/stairspeedtest CMakeFiles/stairspeedtest.dir/src/*.o -static \
    -L/usr/local/lib \
    -Wl,--start-group \
    -lpcre2-8 -levent -lyaml-cpp -lPNGwriter -lpng \
    -lfreetype -lharfbuzz -lgraphite2 -lglib-2.0 -lintl \
    -lcurl -lssl -lcrypto -lz -lbz2 -lbrotlidec -lbrotlicommon \
    -lstdc++ \
    -Wl,--end-group \
    -ldl -lpthread -O3 -s

chmod +rx base/stairspeedtest base/*.sh
chmod +r base/*
