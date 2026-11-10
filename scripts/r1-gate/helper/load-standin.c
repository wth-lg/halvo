#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

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
    fprintf(stderr, "usage: load-standin burn SECONDS | sleep SECONDS | deaf SECONDS\n");
    return 64;
}
