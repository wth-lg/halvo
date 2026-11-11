/* wins-pid <pid>: count the windows that the window server lists for one process, by layer,
 * on screen and in all. Prints counts only (no title is read).
 * Build: cc -O2 -o wins-pid wins-pid.c -framework CoreGraphics -framework CoreFoundation */
#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>
#include <stdio.h>
#include <stdlib.h>

static void count(CGWindowListOption opt, const char *tag, int pid) {
    CFArrayRef list = CGWindowListCopyWindowInfo(opt, kCGNullWindowID);
    if (!list) { printf("%s windows=unknown\n", tag); return; }
    int total = 0, layer25 = 0, layer0 = 0, other = 0;
    for (CFIndex i = 0; i < CFArrayGetCount(list); i++) {
        CFDictionaryRef w = CFArrayGetValueAtIndex(list, i);
        int owner = -1, layer = 0;
        CFNumberRef o = CFDictionaryGetValue(w, kCGWindowOwnerPID);
        CFNumberRef l = CFDictionaryGetValue(w, kCGWindowLayer);
        if (o) CFNumberGetValue(o, kCFNumberIntType, &owner);
        if (l) CFNumberGetValue(l, kCFNumberIntType, &layer);
        if (owner != pid) continue;
        total++;
        if (layer == 25) layer25++; else if (layer == 0) layer0++; else other++;
    }
    printf("%s windows=%d layer25_status=%d layer0_normal=%d other_layers=%d\n", tag, total, layer25, layer0, other);
    CFRelease(list);
}

int main(int argc, char **argv) {
    if (argc != 2) return 64;
    int pid = atoi(argv[1]);
    count(kCGWindowListOptionOnScreenOnly, "onscreen", pid);
    count(kCGWindowListOptionAll, "all", pid);
    return 0;
}
