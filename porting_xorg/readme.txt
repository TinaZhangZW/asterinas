1. apt-get source xorg-server
2. apply patches in xorg-server folder
3. build xorg-server and install
For example: 
```bash
rm -rf builddir
meson setup builddir \
	--prefix=/user-de/xorg/installed_dir \
	--libdir=lib \
	-Dxorg=true \
	-Dglamor=true \
	-Dxkb_output_dir=/var/lib/xkb \
	-Doptimization=0 -Ddebug=true -Dudev=false -Dudev_kms=false
ninja -C builddir install
``` 