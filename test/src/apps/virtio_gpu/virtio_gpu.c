// SPDX-License-Identifier: MPL-2.0

#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/ioctl.h>
#include <linux/fb.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#include "../test.h"

#define SYS_GRAPHICS_DIR "/sys/class/graphics"
#define SYS_VIRTIO_GPU_DRIVER "/sys/bus/virtio/drivers/virtio_gpu"
#define SYS_DRM_CLASS "/sys/class/drm"
#define DEV_PREFIX "/dev/"
#define PAGE_SIZE 4096

#define DRM_IOCTL_BASE 'd'
#define DRM_COMMAND_BASE 0x40
#define DRM_IOWR(nr, type) _IOWR(DRM_IOCTL_BASE, nr, type)
#define DRM_IOW(nr, type) _IOW(DRM_IOCTL_BASE, nr, type)

#define DRM_IOCTL_MODE_GETRESOURCES DRM_IOWR(0xA0, struct drm_mode_get_resources)
#define DRM_IOCTL_MODE_GETCRTC DRM_IOWR(0xA1, struct drm_mode_crtc)
#define DRM_IOCTL_MODE_SETCRTC DRM_IOWR(0xA2, struct drm_mode_crtc)
#define DRM_IOCTL_MODE_GETENCODER DRM_IOWR(0xA6, struct drm_mode_get_encoder)
#define DRM_IOCTL_MODE_GETCONNECTOR DRM_IOWR(0xA7, struct drm_mode_get_connector)
#define DRM_IOCTL_MODE_ADDFB DRM_IOWR(0xAE, struct drm_mode_fb_cmd)
#define DRM_IOCTL_MODE_RMFB DRM_IOW(0xAF, struct drm_mode_fb_cmd)
#define DRM_IOCTL_MODE_CREATE_DUMB DRM_IOWR(0xB2, struct drm_mode_create_dumb)
#define DRM_IOCTL_MODE_MAP_DUMB DRM_IOWR(0xB3, struct drm_mode_map_dumb)
#define DRM_IOCTL_MODE_DESTROY_DUMB DRM_IOW(0xB4, struct drm_mode_destroy_dumb)

#define DRM_IOCTL_VIRTGPU_MAP DRM_IOWR(DRM_COMMAND_BASE + 0x00, struct drm_virtgpu_map)
#define DRM_IOCTL_VIRTGPU_EXECBUFFER DRM_IOWR(DRM_COMMAND_BASE + 0x01, struct drm_virtgpu_execbuffer)
#define DRM_IOCTL_VIRTGPU_GETPARAM DRM_IOWR(DRM_COMMAND_BASE + 0x02, struct drm_virtgpu_getparam)
#define DRM_IOCTL_VIRTGPU_RESOURCE_CREATE DRM_IOWR(DRM_COMMAND_BASE + 0x03, struct drm_virtgpu_resource_create)
#define DRM_IOCTL_VIRTGPU_RESOURCE_INFO DRM_IOWR(DRM_COMMAND_BASE + 0x04, struct drm_virtgpu_resource_info)
#define DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST DRM_IOWR(DRM_COMMAND_BASE + 0x05, struct drm_virtgpu_transfer)
#define DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST DRM_IOWR(DRM_COMMAND_BASE + 0x06, struct drm_virtgpu_transfer)
#define DRM_IOCTL_VIRTGPU_WAIT DRM_IOWR(DRM_COMMAND_BASE + 0x07, struct drm_virtgpu_wait)
#define DRM_IOCTL_VIRTGPU_GET_CAPS DRM_IOWR(DRM_COMMAND_BASE + 0x08, struct drm_virtgpu_get_caps)
#define DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB DRM_IOWR(DRM_COMMAND_BASE + 0x09, struct drm_virtgpu_resource_create_blob)
#define DRM_IOCTL_VIRTGPU_CONTEXT_INIT DRM_IOWR(DRM_COMMAND_BASE + 0x0A, struct drm_virtgpu_context_init)
#define DRM_IOCTL_VIRTGPU_RESOURCE_UNREF DRM_IOWR(DRM_COMMAND_BASE + 0x0B, struct drm_virtgpu_resource_unref)

#define VIRTGPU_PARAM_3D_FEATURES 1

struct drm_mode_modeinfo {
	uint32_t clock;
	uint16_t hdisplay;
	uint16_t hsync_start;
	uint16_t hsync_end;
	uint16_t htotal;
	uint16_t hskew;
	uint16_t vdisplay;
	uint16_t vsync_start;
	uint16_t vsync_end;
	uint16_t vtotal;
	uint16_t vscan;
	uint32_t vrefresh;
	uint32_t flags;
	uint32_t type;
	uint8_t name[32];
};

struct drm_mode_get_resources {
	uint64_t fb_id_ptr;
	uint64_t crtc_id_ptr;
	uint64_t connector_id_ptr;
	uint64_t encoder_id_ptr;
	uint32_t count_fbs;
	uint32_t count_crtcs;
	uint32_t count_connectors;
	uint32_t count_encoders;
	uint32_t min_width;
	uint32_t max_width;
	uint32_t min_height;
	uint32_t max_height;
};

struct drm_mode_get_encoder {
	uint32_t encoder_id;
	uint32_t encoder_type;
	uint32_t crtc_id;
	uint32_t possible_crtcs;
	uint32_t possible_clones;
};

struct drm_mode_get_connector {
	uint64_t encoders_ptr;
	uint64_t modes_ptr;
	uint64_t props_ptr;
	uint64_t prop_values_ptr;
	uint32_t count_modes;
	uint32_t count_props;
	uint32_t count_encoders;
	uint32_t encoder_id;
	uint32_t connector_id;
	uint32_t connector_type;
	uint32_t connector_type_id;
	uint32_t connection;
	uint32_t mm_width;
	uint32_t mm_height;
	uint32_t subpixel;
	uint32_t pad;
};

struct drm_mode_crtc {
	uint64_t set_connectors_ptr;
	uint32_t count_connectors;
	uint32_t crtc_id;
	uint32_t fb_id;
	uint32_t x;
	uint32_t y;
	uint32_t gamma_size;
	uint32_t mode_valid;
	struct drm_mode_modeinfo mode;
};

struct drm_mode_fb_cmd {
	uint32_t fb_id;
	uint32_t width;
	uint32_t height;
	uint32_t pitch;
	uint32_t bpp;
	uint32_t depth;
	uint32_t handle;
};

struct drm_mode_create_dumb {
	uint32_t height;
	uint32_t width;
	uint32_t bpp;
	uint32_t flags;
	uint32_t handle;
	uint32_t pitch;
	uint64_t size;
};

struct drm_mode_map_dumb {
	uint32_t handle;
	uint32_t pad;
	uint64_t offset;
};

struct drm_mode_destroy_dumb {
	uint32_t handle;
};

struct drm_virtgpu_map {
	uint64_t offset;
	uint32_t handle;
	uint32_t pad;
};

struct drm_virtgpu_execbuffer {
	uint32_t flags;
	uint32_t size;
	uint64_t command;
	uint64_t fence_id;
	uint32_t ring_idx;
	uint32_t pad;
};

struct drm_virtgpu_getparam {
	uint64_t param;
	uint64_t value;
};

struct drm_virtgpu_resource_create {
	uint32_t target;
	uint32_t format;
	uint32_t bind;
	uint32_t width;
	uint32_t height;
	uint32_t depth;
	uint32_t array_size;
	uint32_t last_level;
	uint32_t nr_samples;
	uint32_t flags;
	uint32_t bo_handle;
	uint32_t res_handle;
	uint64_t size;
};

struct drm_virtgpu_resource_info {
	uint32_t bo_handle;
	uint32_t res_handle;
	uint32_t size;
	uint32_t stride;
};

struct drm_virtgpu_rect {
	uint32_t x;
	uint32_t y;
	uint32_t w;
	uint32_t h;
};

struct drm_virtgpu_transfer {
	uint32_t bo_handle;
	uint32_t res_handle;
	uint32_t level;
	uint32_t stride;
	uint32_t layer_stride;
	struct drm_virtgpu_rect box_;
	uint64_t offset;
};

struct drm_virtgpu_wait {
	uint32_t handle;
	uint32_t flags;
};

struct drm_virtgpu_get_caps {
	uint64_t caps;
	uint32_t size;
	uint32_t pad;
};

struct drm_virtgpu_resource_create_blob {
	uint32_t blob_mem;
	uint32_t blob_flags;
	uint32_t blob_id;
	uint32_t pad;
	uint64_t size;
	uint32_t bo_handle;
	uint32_t res_handle;
};

struct drm_virtgpu_context_init {
	uint32_t ctx_id;
	uint32_t num_capsets;
	uint64_t capsets;
};

struct drm_virtgpu_resource_unref {
	uint32_t res_handle;
	uint32_t pad;
};

static int fb_fd = -1;
static size_t fb_smem_len;
static uint32_t fb_xres;
static uint32_t fb_yres;
static uint32_t fb_bpp;
static uint32_t fb_line_len;
static char fb_dev_path[PATH_MAX];
static char fb_name[128];
static bool fb_available;

static int drm_fd = -1;
static bool drm_available;
static char drm_dev_path[PATH_MAX];

static bool contains_virtio(const char *name)
{
	char lower[128];
	size_t len = strnlen(name, sizeof(lower) - 1);
	for (size_t i = 0; i < len; ++i) {
		char c = name[i];
		if (c >= 'A' && c <= 'Z') {
			c = (char)(c - 'A' + 'a');
		}
		lower[i] = c;
	}
	lower[len] = '\0';
	return strstr(lower, "virtio") != NULL;
}

static bool read_text_file(const char *path, char *buf, size_t buf_len)
{
	int fd = open(path, O_RDONLY);
	if (fd < 0) {
		return false;
	}
	ssize_t n = read(fd, buf, buf_len - 1);
	close(fd);
	if (n <= 0) {
		return false;
	}
	buf[n] = '\0';
	while (n > 0 && (buf[n - 1] == '\n' || buf[n - 1] == '\r')) {
		buf[n - 1] = '\0';
		n--;
	}
	return true;
}

static bool find_virtio_fb(char *dev_path, size_t dev_len, char *name_buf,
			  size_t name_len)
{
	DIR *dir = opendir(SYS_GRAPHICS_DIR);
	if (!dir) {
		return false;
	}

	struct dirent *ent;
	while ((ent = readdir(dir)) != NULL) {
		if (strncmp(ent->d_name, "fb", 2) != 0) {
			continue;
		}
		char name_path[PATH_MAX];
		snprintf(name_path, sizeof(name_path), "%s/%s/name",
			 SYS_GRAPHICS_DIR, ent->d_name);
		char name[128];
		if (!read_text_file(name_path, name, sizeof(name))) {
			continue;
		}
		if (!contains_virtio(name)) {
			continue;
		}
		snprintf(dev_path, dev_len, "%s%s", DEV_PREFIX, ent->d_name);
		if (name_len > 0) {
			size_t copy_len = strnlen(name, name_len - 1);
			memcpy(name_buf, name, copy_len);
			name_buf[copy_len] = '\0';
		}
		closedir(dir);
		return true;
	}

	closedir(dir);
	return false;
}

static bool virtio_driver_has_devices(void)
{
	DIR *dir = opendir(SYS_VIRTIO_GPU_DRIVER);
	if (!dir) {
		return false;
	}
	struct dirent *ent;
	while ((ent = readdir(dir)) != NULL) {
		if (strncmp(ent->d_name, "virtio", 6) == 0) {
			closedir(dir);
			return true;
		}
	}
	closedir(dir);
	return false;
}

static bool is_virtio_gpu_drm_present(void)
{
	DIR *dir = opendir(SYS_DRM_CLASS);
	if (!dir) {
		return false;
	}

	struct dirent *ent;
	while ((ent = readdir(dir)) != NULL) {
		if (strncmp(ent->d_name, "card", 4) != 0) {
			continue;
		}
		char driver_path[PATH_MAX];
		snprintf(driver_path, sizeof(driver_path), "%s/%s/device/driver",
			 SYS_DRM_CLASS, ent->d_name);
		char link_buf[PATH_MAX];
		ssize_t link_len = readlink(driver_path, link_buf,
					    sizeof(link_buf) - 1);
		if (link_len <= 0) {
			continue;
		}
		link_buf[link_len] = '\0';
		if (strstr(link_buf, "virtio_gpu") != NULL) {
			closedir(dir);
			return true;
		}
	}

	closedir(dir);
	return false;
}

static bool find_virtio_drm(char *dev_path, size_t dev_len)
{
	DIR *dir = opendir(SYS_DRM_CLASS);
	if (!dir) {
		return false;
	}

	struct dirent *ent;
	while ((ent = readdir(dir)) != NULL) {
		if (strncmp(ent->d_name, "card", 4) != 0) {
			continue;
		}
		char driver_path[PATH_MAX];
		snprintf(driver_path, sizeof(driver_path), "%s/%s/device/driver",
			 SYS_DRM_CLASS, ent->d_name);
		char link_buf[PATH_MAX];
		ssize_t link_len = readlink(driver_path, link_buf,
					    sizeof(link_buf) - 1);
		if (link_len <= 0) {
			continue;
		}
		link_buf[link_len] = '\0';
		if (strstr(link_buf, "virtio_gpu") == NULL) {
			continue;
		}
		snprintf(dev_path, dev_len, "/dev/dri/%s", ent->d_name);
		closedir(dir);
		return true;
	}

	closedir(dir);
	return false;
}

FN_SETUP(open_virtio_gpu)
{
	if (!find_virtio_fb(fb_dev_path, sizeof(fb_dev_path), fb_name,
			     sizeof(fb_name))) {
		if (is_virtio_gpu_drm_present()) {
			fprintf(stderr,
				"virtio-gpu present (DRM), but no fbdev node found; "
				"skipping fbdev-based tests\n");
			return;
		}
		fprintf(stderr,
			"virtio-gpu tests skipped: no virtio-gpu device found\n");
		return;
	}

	fb_fd = open(fb_dev_path, O_RDWR);
	if (fb_fd < 0) {
		fprintf(stderr,
			"fatal error: open('%s') failed: %s\n",
			fb_dev_path, strerror(errno));
		exit(EXIT_FAILURE);
	}

	struct fb_fix_screeninfo fix_info;
	struct fb_var_screeninfo var_info;

	CHECK_WITH(ioctl(fb_fd, FBIOGET_FSCREENINFO, &fix_info),
		   _ret == 0 && fix_info.smem_len != 0);
	CHECK_WITH(ioctl(fb_fd, FBIOGET_VSCREENINFO, &var_info),
		   _ret == 0 && var_info.xres != 0);

	fb_smem_len = fix_info.smem_len;
	fb_line_len = fix_info.line_length;
	fb_xres = var_info.xres;
	fb_yres = var_info.yres;
	fb_bpp = var_info.bits_per_pixel;
	fb_available = true;

	fprintf(stderr, "virtio-gpu framebuffer: %s (%s) %ux%u %ubpp\n",
		fb_dev_path, fb_name, fb_xres, fb_yres, fb_bpp);
}
END_SETUP()

FN_TEST(fb_info_sanity)
{
	if (!fb_available) {
		fprintf(stderr, "fbdev tests skipped: no fbdev node\n");
		return;
	}
	TEST_RES(fb_xres, _ret > 0);
	TEST_RES(fb_yres, _ret > 0);
	TEST_RES(fb_bpp, _ret > 0 && _ret <= 64);
	TEST_RES(fb_line_len, _ret >= (fb_xres * (fb_bpp / 8)));
	TEST_RES(fb_smem_len, _ret >= (size_t)fb_line_len * fb_yres);
}
END_TEST()

FN_TEST(mmap_write_readback)
{
	if (!fb_available) {
		fprintf(stderr, "fbdev tests skipped: no fbdev node\n");
		return;
	}
	size_t map_len = fb_smem_len;
	if (map_len > PAGE_SIZE) {
		map_len = PAGE_SIZE;
	}

	uint8_t *mapped = TEST_SUCC((uint8_t *)mmap(
		NULL, map_len, PROT_READ | PROT_WRITE, MAP_SHARED, fb_fd, 0));

	for (size_t i = 0; i < map_len; ++i) {
		mapped[i] = (uint8_t)(i ^ 0x5a);
	}

	bool matched = true;
	for (size_t i = 0; i < map_len; ++i) {
		if (mapped[i] != (uint8_t)(i ^ 0x5a)) {
			matched = false;
			break;
		}
	}
	TEST_RES(matched, _ret == true);

	TEST_RES(munmap(mapped, map_len), _ret == 0);
}
END_TEST()

FN_TEST(virtio_driver_binding)
{
	if (!virtio_driver_has_devices()) {
		fprintf(stderr,
			"virtio-gpu driver binding check skipped: %s missing or empty\n",
			SYS_VIRTIO_GPU_DRIVER);
		return;
	}
	TEST_RES(virtio_driver_has_devices(), _ret == true);
}
END_TEST()

FN_SETUP(open_virtio_drm)
{
	if (!find_virtio_drm(drm_dev_path, sizeof(drm_dev_path))) {
		fprintf(stderr,
			"DRM tests skipped: no virtio-gpu DRM device found\n");
		return;
	}

	drm_fd = open(drm_dev_path, O_RDWR | O_CLOEXEC);
	if (drm_fd < 0) {
		fprintf(stderr,
			"fatal error: open('%s') failed: %s\n",
			drm_dev_path, strerror(errno));
		return;
	}

	drm_available = true;
}
END_SETUP()

FN_TEST(drm_mode_setting)
{
	if (!drm_available) {
		fprintf(stderr, "DRM mode setting skipped: no DRM device\n");
		return;
	}

	const char *modeset_env = getenv("VIRTIO_GPU_MODESET");
	if (!modeset_env || strcmp(modeset_env, "1") != 0) {
		fprintf(stderr,
			"DRM mode setting skipped: set VIRTIO_GPU_MODESET=1 to enable\n");
		return;
	}

	struct drm_mode_get_resources res = {0};
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_GETRESOURCES, &res));
	if (res.count_connectors == 0 || res.count_crtcs == 0) {
		fprintf(stderr, "DRM mode setting skipped: no connectors/crtcs\n");
		return;
	}

	uint32_t *connector_ids = calloc(res.count_connectors, sizeof(uint32_t));
	uint32_t *encoder_ids = calloc(res.count_encoders, sizeof(uint32_t));
	uint32_t *crtc_ids = calloc(res.count_crtcs, sizeof(uint32_t));
	if (!connector_ids || !crtc_ids) {
		fprintf(stderr, "DRM mode setting skipped: allocation failed\n");
		free(connector_ids);
		free(encoder_ids);
		free(crtc_ids);
		return;
	}

	res.connector_id_ptr = (uint64_t)(uintptr_t)connector_ids;
	res.encoder_id_ptr = (uint64_t)(uintptr_t)encoder_ids;
	res.crtc_id_ptr = (uint64_t)(uintptr_t)crtc_ids;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_GETRESOURCES, &res));

	struct drm_mode_modeinfo mode = {0};
	uint32_t connector_id = 0;
	uint32_t encoder_id = 0;
	uint32_t crtc_id = 0;

	for (uint32_t i = 0; i < res.count_connectors; ++i) {
		struct drm_mode_get_connector conn = {0};
		conn.connector_id = connector_ids[i];
		TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_GETCONNECTOR, &conn));
		if (conn.count_modes == 0) {
			continue;
		}
		struct drm_mode_modeinfo *modes =
			calloc(conn.count_modes, sizeof(struct drm_mode_modeinfo));
		uint32_t *encs =
			calloc(conn.count_encoders, sizeof(uint32_t));
		if (!modes || !encs) {
			free(modes);
			free(encs);
			continue;
		}
		conn.modes_ptr = (uint64_t)(uintptr_t)modes;
		conn.encoders_ptr = (uint64_t)(uintptr_t)encs;
		TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_GETCONNECTOR, &conn));
		if (conn.count_modes == 0) {
			free(modes);
			free(encs);
			continue;
		}
		mode = modes[0];
		connector_id = conn.connector_id;
		if (conn.encoder_id != 0) {
			encoder_id = conn.encoder_id;
		} else if (conn.count_encoders > 0) {
			encoder_id = encs[0];
		}
		free(modes);
		free(encs);
		break;
	}

	if (connector_id == 0 || encoder_id == 0) {
		fprintf(stderr, "DRM mode setting skipped: no usable connector\n");
		free(connector_ids);
		free(encoder_ids);
		free(crtc_ids);
		return;
	}

	struct drm_mode_get_encoder enc = {0};
	enc.encoder_id = encoder_id;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_GETENCODER, &enc));
	crtc_id = enc.crtc_id;
	if (crtc_id == 0 && res.count_crtcs > 0) {
		for (uint32_t i = 0; i < res.count_crtcs; ++i) {
			if (enc.possible_crtcs & (1u << i)) {
				crtc_id = crtc_ids[i];
				break;
			}
		}
	}
	if (crtc_id == 0) {
		fprintf(stderr, "DRM mode setting skipped: no usable crtc\n");
		free(connector_ids);
		free(encoder_ids);
		free(crtc_ids);
		return;
	}

	struct drm_mode_create_dumb create = {0};
	create.width = mode.hdisplay;
	create.height = mode.vdisplay;
	create.bpp = 32;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_CREATE_DUMB, &create));

	struct drm_mode_map_dumb map_dumb = {0};
	map_dumb.handle = create.handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_MAP_DUMB, &map_dumb));

	uint32_t *fb_mem = mmap(NULL, create.size, PROT_READ | PROT_WRITE, MAP_SHARED, drm_fd, map_dumb.offset);
	if (fb_mem == MAP_FAILED) {
		fprintf(stderr, "mmap failed: %s\n", strerror(errno));
		free(connector_ids);
		free(encoder_ids);
		free(crtc_ids);
		return;
	}

	// Fill framebuffer with red color
	for (size_t i = 0; i < create.size / 4; ++i) {
		fb_mem[i] = 0xFFFF0000; // ARGB: full alpha, red
	}

	struct drm_mode_fb_cmd fb = {0};
	fb.width = create.width;
	fb.height = create.height;
	fb.pitch = create.pitch;
	fb.bpp = 32;
	fb.depth = 24;
	fb.handle = create.handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_ADDFB, &fb));

	struct drm_mode_crtc crtc = {0};
	crtc.set_connectors_ptr = (uint64_t)(uintptr_t)&connector_id;
	crtc.count_connectors = 1;
	crtc.crtc_id = crtc_id;
	crtc.fb_id = fb.fb_id;
	crtc.x = 0;
	crtc.y = 0;
	crtc.mode_valid = 1;
	crtc.mode = mode;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_MODE_SETCRTC, &crtc));

	sleep(2); // Show the color for 2 seconds

	munmap(fb_mem, create.size);

	ioctl(drm_fd, DRM_IOCTL_MODE_RMFB, &fb);
	{
		struct drm_mode_destroy_dumb destroy = {0};
		destroy.handle = create.handle;
		ioctl(drm_fd, DRM_IOCTL_MODE_DESTROY_DUMB, &destroy);
	}

	free(connector_ids);
	free(encoder_ids);
	free(crtc_ids);
}
END_TEST()

FN_TEST(virtio_gpu_ioctls)
{
	if (!drm_available) {
		fprintf(stderr, "virtio-gpu ioctls skipped: no DRM device\n");
		return;
	}

	struct drm_virtgpu_resource_create create = {0};
	create.width = 64;
	create.height = 64;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_RESOURCE_CREATE, &create));

	struct drm_virtgpu_map map = {0};
	map.handle = create.res_handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_MAP, &map));

	struct drm_virtgpu_resource_info info = {0};
	info.res_handle = create.res_handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_RESOURCE_INFO, &info));

	struct drm_virtgpu_transfer transfer = {0};
	transfer.bo_handle = create.bo_handle;
	transfer.box_.w = create.width;
	transfer.box_.h = create.height;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST, &transfer));
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST, &transfer));

	struct drm_virtgpu_getparam getparam = {0};
	getparam.param = VIRTGPU_PARAM_3D_FEATURES;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_GETPARAM, &getparam));

	struct drm_virtgpu_execbuffer exec = {0};
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_EXECBUFFER, &exec));

	struct drm_virtgpu_wait wait = {0};
	wait.handle = create.bo_handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_WAIT, &wait));

	struct drm_virtgpu_get_caps caps = {0};
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_GET_CAPS, &caps));

	struct drm_virtgpu_context_init ctx = {0};
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_CONTEXT_INIT, &ctx));

	struct drm_virtgpu_resource_create_blob blob = {0};
	TEST_ERRNO(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB, &blob), ENOTTY);

	struct drm_virtgpu_resource_unref unref = {0};
	unref.res_handle = create.res_handle;
	TEST_SUCC(ioctl(drm_fd, DRM_IOCTL_VIRTGPU_RESOURCE_UNREF, &unref));
}
END_TEST()

FN_SETUP(close_virtio_gpu)
{
	if (fb_available) {
		CHECK(close(fb_fd));
	}
}
END_SETUP()

FN_SETUP(close_virtio_drm)
{
	if (drm_available) {
		CHECK(close(drm_fd));
	}
}
END_SETUP()
