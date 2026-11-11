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

static const char *flag_value(int argc, char **argv, const char *flag) {
    for (int i = 1; i + 1 < argc; i++) {
        if (strcmp(argv[i], flag) == 0) {
            return argv[i + 1];
        }
    }
    return NULL;
}

static int has_word(int argc, char **argv, const char *word) {
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], word) == 0) {
            return 1;
        }
    }
    return 0;
}

static int write_file(const char *dir, const char *name, const char *text) {
    char path[4096];
    snprintf(path, sizeof path, "%s/%s", dir, name);
    FILE *file = fopen(path, "w");
    if (file == NULL) {
        return 1;
    }
    fputs(text, file);
    return fclose(file);
}

static int capture_standin(int argc, char **argv) {
    const char *scenario = getenv("STANDIN_SCENARIO");
    const char *dir = flag_value(argc, argv, "--store-dir");
    if (scenario == NULL) {
        scenario = "ok";
    }
    if (dir == NULL || strcmp(scenario, "exit-64") == 0) {
        fprintf(stderr, "usage: halvo-capture --timed-run SECONDS --store-dir DIR [--hold-secure-input] [--switch NAME=on|off]...\n");
        return 64;
    }
    if (strcmp(scenario, "exit-1") == 0) {
        fprintf(stderr, "halvo-capture: the stand-in was told to fail\n");
        return 1;
    }
    if (strcmp(argv[1], "--verify-store") == 0) {
        const char *marker = flag_value(argc, argv, "--marker");
        if (marker == NULL) {
            fprintf(stderr, "usage: halvo-capture --verify-store --store-dir DIR --marker MARKER\n");
            return 64;
        }
        if (strcmp(scenario, "no-store-files") != 0 &&
            (write_file(dir, "store.db", "ciphertext\n") != 0 || write_file(dir, "store.key", "wrapped\n") != 0)) {
            return 1;
        }
        if (strcmp(scenario, "leak-on-stdout") == 0) {
            printf("debug: wrote %s\n", marker);
        }
        if (strcmp(scenario, "leak-in-dir") == 0 && write_file(dir, "plain.txt", marker) != 0) {
            return 1;
        }
        if (strcmp(scenario, "leak-on-stderr") == 0) {
            fprintf(stderr, "debug: wrote %s\n", marker);
        }
        printf("store rows=%d secure_delete=1 temp_store=2 fts_secure_delete=1 cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok unwrap_ms=3 prompt=none\n",
               strcmp(scenario, "rows-0") == 0 ? 0 : 2000);
        return strcmp(scenario, "report-then-exit-1") == 0 ? 1 : 0;
    }
    if (strcmp(argv[1], "--verify-deletion") == 0) {
        printf("segment=42 shred=ok file_gone=yes decryptable=no sibling_readable=yes remaining_keys=1 wal_bytes=0 delete_all_keys=2 root_destroyed=%s store_sealed=yes\n",
               strcmp(scenario, "root-kept") == 0 ? "no" : "yes");
        return strcmp(scenario, "report-then-exit-1") == 0 ? 1 : 0;
    }
    if (strcmp(argv[1], "--timed-run") == 0 && argc >= 3) {
        int held = has_word(argc, argv, "--hold-secure-input");
        int screen_on = has_word(argc, argv, "screen_activity=on") && has_word(argc, argv, "--switch");
        int screen_failed = strcmp(scenario, "screen-failed") == 0;
        int input_failed = strcmp(scenario, "input-failed") == 0;
        int typed = strcmp(scenario, "typed") == 0;
        if (getenv("STANDIN_NEEDS_HOLD") != NULL && !held) {
            fprintf(stderr, "usage: halvo-capture --timed-run SECONDS --store-dir DIR --hold-secure-input\n");
            return 64;
        }
        if (strcmp(scenario, "hang") == 0) {
            idle(20);
        }
        if (strcmp(scenario, "ends-early") == 0) {
            idle(0.3);
        } else if (strcmp(scenario, "busy") == 0) {
            burn(atof(argv[2]));
        } else if (strcmp(scenario, "big") == 0) {
            hold_mapped(64, atof(argv[2]), 0);
        } else {
            idle(atof(argv[2]));
        }
        if (strcmp(scenario, "grows") == 0) {
            char megabyte[4097];
            memset(megabyte, 'x', 4096);
            megabyte[4096] = 0;
            FILE *grown = NULL;
            char path[4096];
            snprintf(path, sizeof path, "%s/grown.bin", dir);
            grown = fopen(path, "w");
            if (grown == NULL) {
                return 1;
            }
            for (int i = 0; i < 256; i++) {
                fputs(megabyte, grown);
            }
            fclose(grown);
        }
        if (screen_failed) {
            fprintf(stderr, "halvo-capture: source screen did not start: the capture permission for ScreenFrame is not granted\n");
        }
        if (input_failed) {
            fprintf(stderr, "halvo-capture: source input did not start: event tap creation failed\n");
        }
        printf("modality=screen_frame count=%d\n", screen_on && !screen_failed ? 12 : 0);
        printf("modality=keystroke count=%d\n", typed && !held ? 5 : 0);
        printf("modality=pointer count=%d\n", typed ? 7 : 0);
        printf("modality=foreground_app count=2\n");
        printf("modality=window_title count=1\n");
        printf("secure_input_keystrokes=%d\n", strcmp(scenario, "keystroke-while-held") == 0 ? 3 : 0);
        return strcmp(scenario, "report-then-exit-1") == 0 ? 1 : 0;
    }
    fprintf(stderr, "usage: halvo-capture --timed-run | --verify-store | --verify-deletion\n");
    return 64;
}

int main(int argc, char **argv) {
    if (argc >= 2 && strncmp(argv[1], "--", 2) == 0) {
        return capture_standin(argc, argv);
    }
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
