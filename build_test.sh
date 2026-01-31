## make test cases
#cd test
#make x86_64_pkgs

## run test cases
make run_kernel ENABLE_BASIC_TEST=true

make run_kernel ENABLE_BASIC_TEST=true LOG_LEVEL=warn
VIRTIO_GPU_MODESET=1 /test/virtio_gpu/virtio_gpu
