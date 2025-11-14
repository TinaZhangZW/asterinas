// SPDX-License-Identifier: MPL-2.0

#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/inotify.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <sys/ioctl.h>

static void die(const char *msg)
{
	perror(msg);
	exit(1);
}

static void ensure_dir(const char *path)
{
	if (mkdir(path, 0700) < 0 && errno != EEXIST)
		die("mkdir");
}

static void create_file(const char *path)
{
	int fd = open(path, O_CREAT | O_WRONLY, 0600);
	if (fd < 0)
		die("open");
	close(fd);
}

static void remove_file(const char *path)
{
	if (unlink(path) < 0 && errno != ENOENT)
		die("unlink");
}

// Check if a pointer is aligned to a given alignment
static bool is_aligned(const void *ptr, size_t align)
{
	return ((uintptr_t)ptr % align) == 0;
}

// Round up to alignment
static size_t align_up(size_t size, size_t align)
{
	return (size + align - 1) & ~(align - 1);
}

int main(void)
{
	const char *dir = "inotify_align_tmp";
	
	// Clean up from previous runs
	remove_file("inotify_align_tmp/file1");
	remove_file("inotify_align_tmp/file2");
	remove_file("inotify_align_tmp/file3");
	remove_file("inotify_align_tmp/a");
	remove_file("inotify_align_tmp/very_long_filename_that_requires_padding");
	if (rmdir(dir) < 0 && errno != ENOENT)
		die("rmdir");
	
	ensure_dir(dir);

	int ifd = inotify_init1(0);
	if (ifd < 0)
		die("inotify_init1");

	int wd = inotify_add_watch(ifd, dir, IN_CREATE | IN_DELETE);
	if (wd < 0)
		die("inotify_add_watch");

	printf("Testing inotify event alignment...\n");
	
	// Create files with different name lengths to test alignment
	// File 1: short name (1 char)
	create_file("inotify_align_tmp/a");
	
	// File 2: medium name (5 chars)
	create_file("inotify_align_tmp/file1");
	
	// File 3: long name (40 chars)
	create_file("inotify_align_tmp/very_long_filename_that_requires_padding");
	
	// Delete one file to test deletion events
	remove_file("inotify_align_tmp/file1");

	// Wait a bit for events to be queued
	usleep(100000); // 100ms

	char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
	ssize_t len = read(ifd, buf, sizeof(buf));
	if (len < 0)
		die("read");

	printf("Read %zd bytes of events\n", len);

	const size_t event_struct_size = sizeof(struct inotify_event);
	printf("sizeof(struct inotify_event) = %zu\n", event_struct_size);
	printf("__alignof__(struct inotify_event) = %zu\n", __alignof__(struct inotify_event));

	// Verify buffer is aligned
	if (!is_aligned(buf, __alignof__(struct inotify_event))) {
		fprintf(stderr, "ERROR: Buffer is not aligned to struct boundary\n");
		return 1;
	}
	printf("Buffer is properly aligned\n");

	// Parse events and verify alignment
	size_t offset = 0;
	int event_count = 0;
	
	while (offset < (size_t)len) {
		struct inotify_event *event = (struct inotify_event *)(buf + offset);
		
		// Verify event pointer is aligned
		if (!is_aligned(event, __alignof__(struct inotify_event))) {
			fprintf(stderr, 
				"ERROR: Event #%d at offset %zu is not aligned (ptr=%p, align=%zu)\n",
				event_count, offset, (void *)event, __alignof__(struct inotify_event));
			return 1;
		}

		// Calculate expected event size
		size_t name_len = event->len;
		size_t header_size = sizeof(struct inotify_event);
		size_t expected_name_padded = 0;
		
		if (name_len > 0) {
			// Name length should be rounded up to event_struct_size
			expected_name_padded = align_up(name_len, event_struct_size);
		}
		
		size_t expected_event_size = header_size + expected_name_padded;
		
		// Calculate actual event size (for next event offset)
		size_t actual_event_size = header_size;
		if (name_len > 0) {
			actual_event_size += name_len;
			// The name should be padded to align to event_struct_size
			size_t name_padding = align_up(name_len, event_struct_size) - name_len;
			actual_event_size += name_padding;
		}

		printf("\nEvent #%d:\n", event_count);
		printf("  Offset: %zu\n", offset);
		printf("  Pointer: %p (aligned: %s)\n", 
		       (void *)event, 
		       is_aligned(event, __alignof__(struct inotify_event)) ? "yes" : "no");
		printf("  wd: %d\n", event->wd);
		printf("  mask: 0x%x\n", event->mask);
		printf("  cookie: %u\n", event->cookie);
		printf("  len: %u\n", event->len);
		if (event->len > 0) {
			printf("  name: '%s'\n", event->name);
			printf("  name_len: %zu\n", name_len);
			printf("  expected_name_padded: %zu\n", expected_name_padded);
		}
		printf("  expected_event_size: %zu\n", expected_event_size);

		// Verify the next event offset
		size_t next_offset = offset + actual_event_size;
		
		// Check if we have enough data for next event
		if (next_offset <= (size_t)len) {
			// Verify next event would be aligned
			if (next_offset < (size_t)len) {
				struct inotify_event *next_event = (struct inotify_event *)(buf + next_offset);
				if (!is_aligned(next_event, __alignof__(struct inotify_event))) {
					fprintf(stderr,
						"ERROR: Next event at offset %zu would not be aligned (ptr=%p)\n",
						next_offset, (void *)next_event);
					return 1;
				}
				printf("  Next event would be aligned at offset %zu\n", next_offset);
			}
		}

		offset = next_offset;
		event_count++;
	}

	printf("\nTotal events parsed: %d\n", event_count);
	
	if (event_count < 3) {
		fprintf(stderr, "ERROR: Expected at least 3 events, got %d\n", event_count);
		return 1;
	}

	// Cleanup
	remove_file("inotify_align_tmp/a");
	remove_file("inotify_align_tmp/very_long_filename_that_requires_padding");
	close(ifd);
	if (rmdir(dir) < 0)
		die("rmdir");

	printf("\nAll alignment tests passed!\n");
	return 0;
}

