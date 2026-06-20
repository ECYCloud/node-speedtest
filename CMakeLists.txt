cmake_minimum_required(VERSION 3.10)
project(stairspeedtest LANGUAGES CXX)
SET(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CMAKE_SOURCE_DIR}/cmake/")
INCLUDE_DIRECTORIES("${CMAKE_SOURCE_DIR}/include/")
IF(NOT CMAKE_BUILD_TYPE)
    SET(CMAKE_BUILD_TYPE Release)
ENDIF()
SET(CMAKE_CXX_STANDARD 17)
ADD_DEFINITIONS(-Wall -Wextra -Wno-unused-parameter -Wno-unused-result)

# 桌面端 sidecar 引擎模式:加入 separated/src/ 的 HTTP 服务源文件，链接 libevent。
# 默认 OFF —— 主项目仍是纯 CLI 二进制，与本目录无任何依赖关系。
OPTION(BUILD_WEBSERVER_ENGINE "Build the /web sidecar engine for the Tauri desktop app (depends on libevent)" OFF)

# ---- 安全硬化(GCC / Clang)---------------------------------------------------
# 项目历史出过 CVE 的库(OpenSSL / PCRE2 / libpng / yaml-cpp 等)被静态/动态链
# 进二进制，单纯靠 -Wall 接不住缓冲区溢出 / UAF 类问题。这里加运行时硬化：
#   -fstack-protector-strong  栈金丝雀
#   -D_FORTIFY_SOURCE=2       libc 检测越界(配合 -O 才生效，所以放在 Release 上)
#   -fno-strict-aliasing      避免依赖严格别名规则的优化 UB(项目里多处 reinterpret_cast)
# 链接器:
#   -Wl,--dynamicbase --high-entropy-va --nxcompat   PE ASLR/DEP 标记
#   -Wl,-z,relro -Wl,-z,now                          ELF 立刻绑定 + 只读 GOT
# Linux/macOS 才发 -fPIE -pie;MinGW 不需要(PE 走 --dynamicbase)
IF(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    ADD_COMPILE_OPTIONS(-fstack-protector-strong -fno-strict-aliasing)
    # _FORTIFY_SOURCE 只在 -O1+ 下有意义;Release 默认 -O3，OK
    ADD_COMPILE_DEFINITIONS($<$<NOT:$<CONFIG:Debug>>:_FORTIFY_SOURCE=2>)
    IF(WIN32)
        ADD_LINK_OPTIONS(-Wl,--dynamicbase -Wl,--nxcompat -Wl,--high-entropy-va)
    ELSE()
        ADD_COMPILE_OPTIONS(-fPIE)
        ADD_LINK_OPTIONS(-pie -Wl,-z,relro -Wl,-z,now)
    ENDIF()
ENDIF()
# ---- 安全硬化 end -----------------------------------------------------------

INCLUDE(CheckCXXSourceCompiles)
CHECK_CXX_SOURCE_COMPILES(
"
#include<string>
void main(){std::to_string(0);}
" HAVE_TO_STRING)

INCLUDE(CheckSymbolExists)
CHECK_SYMBOL_EXISTS(MSG_NOSIGNAL "sys/socket.h" HAVE_MSG_NOSIGNAL)

IF(APPLE)
    ADD_DEFINITIONS(-D_MACOS)
ENDIF()

IF(HAVE_TO_STRING)
    ADD_DEFINITIONS(-DHAVE_TO_STRING)
ENDIF()

IF(HAVE_MSG_NOSIGNAL)
	ADD_DEFINITIONS(-DHAVE_MSG_NOSIGNAL)
ENDIF()

ADD_EXECUTABLE(stairspeedtest
	src/confbuild.cpp
	src/geoip.cpp
	src/emoji.cpp
	src/logger.cpp
	src/main.cpp
	src/md5.cpp
	src/misc.cpp
	src/multithread_test.cpp
	src/ntt.cpp
	src/printmsg.cpp
	src/processes.cpp
	src/renderer.cpp
	src/renderer_v2.cpp
	src/rulematch.cpp
	src/socket.cpp
	src/speedtestutil.cpp
	src/subconv.cpp
	src/webget.cpp
	$<$<BOOL:${BUILD_WEBSERVER_ENGINE}>:${CMAKE_SOURCE_DIR}/separated/src/webgui_wrapper.cpp>
	$<$<BOOL:${BUILD_WEBSERVER_ENGINE}>:${CMAKE_SOURCE_DIR}/separated/src/webserver_libevent.cpp>)
INCLUDE_DIRECTORIES(src)
LINK_DIRECTORIES(${CMAKE_SOURCE_DIR})

FIND_PACKAGE(PkgConfig REQUIRED)

IF(BUILD_WEBSERVER_ENGINE)
	# /web 模式需要 libevent 的 HTTP 服务能力。BUILD_WEBSERVER_ENGINE 宏会让
	# main.cpp / webgui_wrapper.cpp 启用 #ifdef 块,CLI 模式下完全不参与编译。
	TARGET_COMPILE_DEFINITIONS(stairspeedtest PRIVATE BUILD_WEBSERVER_ENGINE)
	INCLUDE_DIRECTORIES(${CMAKE_SOURCE_DIR}/separated/src)
	PKG_CHECK_MODULES(LIBEVENT libevent REQUIRED)
	TARGET_INCLUDE_DIRECTORIES(stairspeedtest PRIVATE ${LIBEVENT_INCLUDE_DIRS})
	# 用 target_link_directories 而不是顶级 LINK_DIRECTORIES。后者是目录级属性,
	# 只对它之后才定义的 target 生效;stairspeedtest target 在更前面已经 add_executable,
	# 顶级 LINK_DIRECTORIES 对它无效。这在 macOS Homebrew(/opt/homebrew/lib 不在默认
	# 搜索路径)上会导致 ld 报 'library event not found',Linux/Windows 因为
	# libevent 在系统默认路径里偶然蒙对。
	TARGET_LINK_DIRECTORIES(stairspeedtest PRIVATE ${LIBEVENT_LIBRARY_DIRS})
	TARGET_LINK_LIBRARIES(stairspeedtest ${LIBEVENT_LIBRARIES})
ENDIF()

SET(THREADS_PREFER_PTHREAD_FLAG ON)
FIND_PACKAGE(Threads REQUIRED)
TARGET_LINK_LIBRARIES(stairspeedtest ${CMAKE_THREAD_LIBS_INIT})

FIND_PACKAGE(CURL 7.54.0 REQUIRED)
LINK_DIRECTORIES(${CURL_LIBRARY_DIRS})
INCLUDE_DIRECTORIES(${CURL_INCLUDE_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest CURL::libcurl)
ADD_DEFINITIONS(-DCURL_STATICLIB)

FIND_PACKAGE(OpenSSL 1.1.0 REQUIRED)
INCLUDE_DIRECTORIES(${OPENSSL_INCLUDE_DIR})
TARGET_LINK_LIBRARIES(stairspeedtest ${OPENSSL_LIBRARIES})

FIND_PACKAGE(Rapidjson REQUIRED)
INCLUDE_DIRECTORIES(${RAPIDJSON_INCLUDE_DIRS})

FIND_PACKAGE(PNG REQUIRED)
INCLUDE_DIRECTORIES(${PNG_INCLUDE_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest ${PNG_LIBRARIES})
ADD_DEFINITIONS(${PNG_DEFINITIONS})

FIND_PACKAGE(Freetype REQUIRED)
INCLUDE_DIRECTORIES(${FREETYPE_INCLUDE_DIR_freetype2})
TARGET_LINK_LIBRARIES(stairspeedtest ${FREETYPE_LIBRARIES})

# HarfBuzz: used for native colour-emoji shaping (flag emoji = RI pair ligature)
PKG_CHECK_MODULES(HARFBUZZ harfbuzz REQUIRED)
FIND_LIBRARY(HARFBUZZ_LIBRARY NAMES harfbuzz PATHS ${HARFBUZZ_LIBRARY_DIRS})
INCLUDE_DIRECTORIES(${HARFBUZZ_INCLUDE_DIRS})
LINK_DIRECTORIES(${HARFBUZZ_LIBRARY_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest ${HARFBUZZ_LIBRARY})

FIND_PACKAGE(PNGwriter 0.7.0 REQUIRED)
INCLUDE_DIRECTORIES(${PNGwriter_INCLUDE_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest PNGwriter::PNGwriter)

PKG_CHECK_MODULES(YAML_CPP yaml-cpp REQUIRED)
FIND_LIBRARY(YAML_CPP_LIBRARY NAMES yaml-cpp yaml-cppd PATHS ${YAML_CPP_LIBRARY_DIRS})
LINK_DIRECTORIES(${YAML_CPP_LIBRARY_DIRS})
INCLUDE_DIRECTORIES(${YAML_CPP_INCLUDE_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest ${YAML_CPP_LIBRARY})
ADD_DEFINITIONS(-DYAML_CPP_STATIC_DEFINE)

FIND_PACKAGE(PCRE2 REQUIRED)
INCLUDE_DIRECTORIES(${PCRE2_INCLUDE_DIRS})
TARGET_LINK_LIBRARIES(stairspeedtest ${PCRE2_LIBRARY})
ADD_DEFINITIONS(-DPCRE2_STATIC)

IF(WIN32)
	TARGET_LINK_LIBRARIES(stairspeedtest wsock32 ws2_32)
ELSE()
	INCLUDE(GNUInstallDirs)
	INSTALL(TARGETS stairspeedtest DESTINATION ${CMAKE_INSTALL_BINDIR})
ENDIF()
