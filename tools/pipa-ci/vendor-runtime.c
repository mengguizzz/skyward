#define _GNU_SOURCE
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <unistd.h>

/* A static launcher: no vendor libraries are loaded before namespace setup. */
int main(int argc, char **argv) {
    int check_security = argc == 3 && !strcmp(argv[1], "--check-security");
    int check = argc == 3 && (!strcmp(argv[1], "--check") || check_security);
    int security = check_security || (argc == 3 && !strcmp(argv[1], "--ramdisk-security")
        && !strcmp(argv[2], "/vendor/bin/qseecomd"));
    int program = check || security ? 2 : 1;
    if (argc <= program || (strncmp(argv[program], "/vendor/bin/", 12)
        && !(check && !strncmp(argv[program], "/vendor/lib64/", 14)))) {
        fprintf(stderr, "usage: pipa-vendor-exec /vendor/bin/PROGRAM [ARGS...]\n");
        return 2;
    }
    if (chdir("/") || unshare(CLONE_NEWNS)
        || mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)) {
        perror("pipa-vendor-exec: isolate");
        return 1;
    }
    /* The firmware partition has an independent /firmware view. Drop inherited
       super mounts only in this child, including the vendor firmware child. */
    const char *points[] = {
        "/vendor/firmware_mnt", "/vendor",
        "/odm", "/mnt/product", "/mnt/system_ext", "/mnt/system",
    };
    for (unsigned i = 0; i < sizeof(points) / sizeof(points[0]); ++i) {
        if (umount2(points[i], MNT_DETACH) && errno != EINVAL && errno != ENOENT) {
            perror(points[i]);
            return 1;
        }
    }
    if (security) {
        /* qsee uses the original boot ramdisk's security stack, including its
           later dlopen calls. Never substitute same-named installed ROM libs. */
        if ((mkdir("/vendor/firmware_mnt", 0755) && errno != EEXIST)
            || mount("/firmware", "/vendor/firmware_mnt", NULL, MS_BIND, NULL)) {
            perror("pipa-vendor-exec: ramdisk security firmware");
            return 1;
        }
    } else {
        if (mount("/skyward/vendor-runtime", "/vendor", NULL, MS_BIND, NULL)
            || mount(NULL, "/vendor", NULL,
                     MS_BIND | MS_REMOUNT | MS_RDONLY | MS_NOSUID | MS_NODEV, NULL)) {
            perror("pipa-vendor-exec: RAM vendor");
            return 1;
        }
    }
    /* /vendor/firmware_mnt is a packaged link to /firmware. No real vendor
       bind, open descriptor or file mapping is retained by this launcher. */
    const char *libraries = getenv("LD_LIBRARY_PATH");
    if ((!libraries || !strcmp(libraries, "/system/lib64"))
        && setenv("LD_LIBRARY_PATH", "/system/lib64:/vendor/lib64:/vendor/lib64/hw", 1)) {
        perror("pipa-vendor-exec: library path");
        return 1;
    }
    if (check) {
        char *list[] = {"/system/bin/linker64", "--list", argv[program], NULL};
        execv(list[0], list);
    } else {
        execv(argv[program], argv + program);
    }
    perror("pipa-vendor-exec: exec");
    return 127;
}
