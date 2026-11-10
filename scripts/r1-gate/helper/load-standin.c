#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec / 1e9;
}

static void burn(double seconds) {
    double end = now() + seconds;
    volatile unsigned long sink = 0;
    while (now() < end) {
        for (int i = 0; i < 100000; i++) {
            sink += (unsigned long)i;
        }
    }
}

static void idle(double seconds) {
    double end = now() + seconds;
    while (now() < end) {
        double left = end - now();
        struct timespec t = {(time_t)left, (long)((left - (double)(time_t)left) * 1e9)};
        nanosleep(&t, NULL);
    }
}

static void touch(volatile unsigned char *bytes, size_t length) {
    for (size_t i = 0; i < length; i += 4096) {
        bytes[i] = (unsigned char)(i >> 12) | 1;
    }
}

static void *burn_briefly(void *unused) {
    (void)unused;
    burn(0.05);
    return NULL;
}

static int hold_mapped(size_t mib, double hold_s, double idle_s) {
    size_t length = mib << 20;
    unsigned char *bytes = mmap(NULL, length, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    if (bytes == MAP_FAILED) {
        return 1;
    }
    touch(bytes, length);
    idle(hold_s);
    munmap(bytes, length);
    idle(idle_s);
    return 0;
}

static int hold_allocated(size_t mib, double hold_s, double idle_s) {
    size_t length = mib << 20;
    unsigned char *bytes = malloc(length);
    if (bytes == NULL) {
        return 1;
    }
    touch(bytes, length);
    idle(hold_s);
    free(bytes);
    idle(idle_s);
    return 0;
}

static int leak(size_t step_mib, int steps, double interval_s) {
    for (int i = 0; i < steps; i++) {
        unsigned char *bytes = malloc(step_mib << 20);
        if (bytes == NULL) {
            return 1;
        }
        touch(bytes, step_mib << 20);
        idle(interval_s);
    }
    return 0;
}

static int short_lived_threads(double seconds) {
    double end = now() + seconds;
    while (now() < end) {
        pthread_t thread;
        if (pthread_create(&thread, NULL, burn_briefly, NULL) != 0) {
            return 1;
        }
        pthread_join(thread, NULL);
    }
    return 0;
}

static int short_lived_children(int count, double each_s) {
    for (int i = 0; i < count; i++) {
        pid_t child = fork();
        if (child < 0) {
            return 1;
        }
        if (child == 0) {
            burn(each_s);
            _exit(0);
        }
        waitpid(child, NULL, 0);
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "burn") == 0) {
        burn(atof(argv[2]));
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "sleep") == 0) {
        idle(atof(argv[2]));
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "deaf") == 0) {
        signal(SIGTERM, SIG_IGN);
        idle(atof(argv[2]));
        return 0;
    }
    if (argc == 5 && strcmp(argv[1], "spike-munmap") == 0) {
        return hold_mapped((size_t)atoi(argv[2]), atof(argv[3]), atof(argv[4]));
    }
    if (argc == 5 && strcmp(argv[1], "spike") == 0) {
        return hold_allocated((size_t)atoi(argv[2]), atof(argv[3]), atof(argv[4]));
    }
    if (argc == 5 && strcmp(argv[1], "leak") == 0) {
        return leak((size_t)atoi(argv[2]), atoi(argv[3]), atof(argv[4]));
    }
    if (argc == 3 && strcmp(argv[1], "short-lived-threads") == 0) {
        return short_lived_threads(atof(argv[2]));
    }
    if (argc == 4 && strcmp(argv[1], "short-lived-children") == 0) {
        return short_lived_children(atoi(argv[2]), atof(argv[3]));
    }
    if (argc == 4 && strcmp(argv[1], "burn-last") == 0) {
        idle(atof(argv[2]));
        burn(atof(argv[3]));
        return 0;
    }
    fprintf(stderr, "usage: load-standin burn S | sleep S | deaf S | spike-munmap MIB HOLD_S IDLE_S | spike MIB HOLD_S IDLE_S | leak STEP_MIB STEPS INTERVAL_S | short-lived-threads S | short-lived-children N EACH_S | burn-last IDLE_S BURN_S\n");
    return 64;
}
