#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

/* Pipa recovery only. Path-stat leaks mount references on retained kernels. */
int fstatat(int dirfd, const char *path, struct stat *st, int flags) {
    const int allowed = AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT | AT_EMPTY_PATH;
    if (!path || (flags & ~allowed) || (!*path && !(flags & AT_EMPTY_PATH)))
        return syscall(SYS_newfstatat, dirfd, path, st, flags);
    if (!*path && dirfd >= 0)
        return syscall(SYS_fstat, dirfd, st);
    const char *name = *path ? path : ".";
    int fd = openat(dirfd, name, O_PATH | O_CLOEXEC
        | ((flags & AT_SYMLINK_NOFOLLOW) ? O_NOFOLLOW : 0));
    if (fd < 0) return -1;
    int rc = syscall(SYS_fstat, fd, st);
    int saved = errno;
    close(fd);
    errno = saved;
    return rc;
}

int stat(const char *path, struct stat *st) {
    return fstatat(AT_FDCWD, path, st, 0);
}

int lstat(const char *path, struct stat *st) {
    return fstatat(AT_FDCWD, path, st, AT_SYMLINK_NOFOLLOW);
}

int fstatat64(int dirfd, const char *path, struct stat64 *st, int flags) {
    _Static_assert(sizeof(struct stat64) == sizeof(struct stat), "aarch64 stat ABI");
    return fstatat(dirfd, path, (struct stat *)st, flags);
}

int stat64(const char *path, struct stat64 *st) {
    return fstatat64(AT_FDCWD, path, st, 0);
}

int lstat64(const char *path, struct stat64 *st) {
    return fstatat64(AT_FDCWD, path, st, AT_SYMLINK_NOFOLLOW);
}
