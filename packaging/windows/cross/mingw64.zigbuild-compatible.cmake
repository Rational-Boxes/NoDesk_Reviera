# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
#
# CMake toolchain for cross-building Windows x86-64 (MinGW-w64, GNU) from Linux, with Fedora's
# mingw64-* packages (sysroot /usr/x86_64-w64-mingw32/sys-root/mingw).
#
# The file name contains "zigbuild" on purpose: wxdragon-sys 0.9 takes its Linux-to-Windows
# cross path (Ninja, this toolchain, the "-Windows" library suffix) only when the toolchain
# file's path contains that word; its other Windows GNU path assumes a Windows host.

set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(TOOLCHAIN_PREFIX x86_64-w64-mingw32)

set(CMAKE_C_COMPILER ${TOOLCHAIN_PREFIX}-gcc)
set(CMAKE_CXX_COMPILER ${TOOLCHAIN_PREFIX}-g++)
set(CMAKE_RC_COMPILER ${TOOLCHAIN_PREFIX}-windres)

set(CMAKE_FIND_ROOT_PATH /usr/${TOOLCHAIN_PREFIX}/sys-root/mingw /usr/${TOOLCHAIN_PREFIX})
# The prefix the cross-build installs its own dependencies in (OpenCASCADE, LuaJIT), so they are
# found as part of the target system (build-cross.sh sets it).
if(DEFINED ENV{REVIERA_CROSS_PREFIX})
  list(APPEND CMAKE_FIND_ROOT_PATH $ENV{REVIERA_CROSS_PREFIX})
endif()
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
