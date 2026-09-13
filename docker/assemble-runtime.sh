#!/bin/bash
set -euo pipefail

# Preserve Ubuntu's library paths, including the architecture-specific loader.
mkdir -p /rootfs/usr/lib /rootfs/usr/lib64 /rootfs/etc/ssl/certs /rootfs/tmp
ln -s usr/lib /rootfs/lib
ln -s usr/lib64 /rootfs/lib64
chmod 1777 /rootfs/tmp
cp -a /app /opt /rootfs/
cp /etc/ssl/certs/ca-certificates.crt /rootfs/etc/ssl/certs/
cp /etc/nsswitch.conf /etc/passwd /etc/group /rootfs/etc/
cp -a --parents /usr/share/glvnd/egl_vendor.d /rootfs/

strip --strip-unneeded /usr/local/bin/node
roots=(/usr/local/bin/node)
# EGL loads Mesa and its software driver with dlopen; ldd on mbgl.node alone
# cannot discover them. Gallium is also loaded dynamically by the DRI driver.
while IFS= read -r -d '' file; do
    roots+=("$file")
done < <(
    find /app/node_modules \( -name '*.node' -o -name '*.so*' \) -print0
    find /usr/lib \( -name libEGL_mesa.so.0 -o -name 'libgallium-*.so' \
        -o -name swrast_dri.so -o -name libnss_files.so.2 -o -name libnss_dns.so.2 \) -print0
)
# The current adapter is static; include its dependencies if that changes.
if readelf -l /opt/extensions/lambda-adapter | grep -q INTERP; then
    roots+=(/opt/extensions/lambda-adapter)
fi
ldd "${roots[@]}" > /tmp/runtime-libraries.txt
if grep -q 'not found' /tmp/runtime-libraries.txt; then
    cat /tmp/runtime-libraries.txt >&2
    exit 1
fi
cp -L --parents "${roots[@]}" /rootfs/
while IFS= read -r library; do
    cp -L --parents "$library" /rootfs/
done < <(awk '/=> \// {print $3} /^[[:space:]]*\// && $1 !~ /:$/ {print $1}' /tmp/runtime-libraries.txt | sort -u)
