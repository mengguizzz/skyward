#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdio.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

/* Test only fresh empty RAM filesystems in a private mount namespace. */
int main(void) {
    if (unshare(CLONE_NEWNS) ||
        mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)) {
        perror("isolate mount namespace");
        return 1;
    }
    const char *operations[] = {
        "none", "stat", "newfstatat", "child-newfstatat", "open-fstat-close", "access"
    };
    int busy = 0;
    for (unsigned i = 0; i < sizeof(operations) / sizeof(operations[0]); ++i) {
        char target[128];
        snprintf(target, sizeof(target), "/tmp/skyward-mount-probe-%ld-%u",
                 (long)getpid(), i);
        if (mkdir(target, 0700)) {
            perror("create probe directory");
            return 1;
        }
        if (mount("tmpfs", target, "tmpfs", MS_NOSUID | MS_NODEV, "size=64k")) {
            perror("mount probe tmpfs");
            rmdir(target);
            return 1;
        }
        struct stat st;
        int rc = 0;
        switch (i) {
        case 0:
            break;
        case 1:
            rc = stat(target, &st);
            break;
        case 2:
            rc = (int)syscall(SYS_newfstatat, AT_FDCWD, target, &st, 0);
            break;
        case 3: {
            fflush(stdout);
            pid_t child = fork();
            if (!child)
                _exit(syscall(SYS_newfstatat, AT_FDCWD, target, &st, 0) ? 1 : 0);
            int status = 0;
            if (child < 0 || waitpid(child, &status, 0) != child ||
                !WIFEXITED(status) || WEXITSTATUS(status)) {
                fprintf(stderr, "child attribute query failed\n");
                rc = -1;
            }
            break;
        }
        case 4: {
            int fd = open(target, O_PATH | O_DIRECTORY | O_CLOEXEC);
            if (fd < 0) {
                rc = -1;
                break;
            }
            rc = (int)syscall(SYS_fstat, fd, &st);
            int saved = errno;
            close(fd);
            errno = saved;
            break;
        }
        case 5:
            rc = access(target, F_OK);
            break;
        }
        int query_error = errno;
        int unmounted = umount2(target, 0);
        int unmount_error = errno;
        if (rc) {
            fprintf(stderr, "%s query failed: %s\n", operations[i], strerror(query_error));
        } else {
            printf("%s: ordinary umount %s\n", operations[i],
                   unmounted ? strerror(unmount_error) : "OK");
        }
        if (unmounted) {
            /* Never detach inherited device mounts, only our empty tmpfs. */
            if (umount2(target, MNT_DETACH)) {
                perror("clean up probe tmpfs");
                return 1;
            }
            if (unmount_error == EBUSY) busy = 1;
        }
        if (rmdir(target)) {
            perror("remove probe directory");
            return 1;
        }
        if (rc || (unmounted && unmount_error != EBUSY)) return 1;
    }
    return busy ? 2 : 0;
}
