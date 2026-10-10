#!/bin/sh
# deb 安装后钩子（R24-c；dh_installdeb 约定）
# 职责：桌面入口 / mime / scheme 注册的缓存刷新（D1 核对项）
set -e

if [ -x /usr/bin/update-desktop-database ]; then
  update-desktop-database -q /usr/share/applications || true
fi
if [ -x /usr/bin/update-mime-database ]; then
  update-mime-database /usr/share/mime > /dev/null || true
fi
# x-scheme-handler/mdcore 注册（desktop-file 面已声明；缓存刷新即可）
if [ -x /usr/bin/gtk-update-icon-cache ]; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi

#DEBHELPER#
