/* si-probe: S7 secure-input probe for the hosted macos-15 runner (run 20261108-halvo-r1-capture).
 * Prints counts and booleans only.
 *   si-probe tap <seconds>    listen-only tap at the HID location, head insert (as the product's
 *                             input source), mask key down, key up, flags changed; prints counts
 *                             by type, split by whether secure input was on when the event arrived
 *   si-probe hold <seconds>   EnableSecureEventInput, report the flag, sleep, DisableSecureEventInput
 *   si-probe post <n>         post n key-down/key-up pairs (key code 0) and n shift flags-changed
 *                             pairs at the HID tap location, 40 ms apart
 *   si-probe counter          HID system state counters for key down, key up, flags changed
 *   si-probe state            listen preflight and the secure-input flag
 * Build: cc -O2 -o si-probe si-probe.c -framework ApplicationServices -framework Carbon */
#include <ApplicationServices/ApplicationServices.h>
#include <Carbon/Carbon.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static long n_down[2], n_up[2], n_flags[2], n_disabled;

static CGEventRef cb(CGEventTapProxy proxy, CGEventType type, CGEventRef ev, void *ctx) {
    (void)proxy; (void)ctx;
    int s = IsSecureEventInputEnabled() ? 1 : 0;
    if (type == kCGEventKeyDown) n_down[s]++;
    else if (type == kCGEventKeyUp) n_up[s]++;
    else if (type == kCGEventFlagsChanged) n_flags[s]++;
    else if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) n_disabled++;
    return ev;
}

static void counters(const char *tag) {
    printf("%s hid_counter key_down=%u key_up=%u flags_changed=%u\n", tag,
           CGEventSourceCounterForEventType(kCGEventSourceStateHIDSystemState, kCGEventKeyDown),
           CGEventSourceCounterForEventType(kCGEventSourceStateHIDSystemState, kCGEventKeyUp),
           CGEventSourceCounterForEventType(kCGEventSourceStateHIDSystemState, kCGEventFlagsChanged));
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc < 2) return 64;
    if (!strcmp(argv[1], "state")) {
        printf("listen_preflight=%s secure_input=%s\n", CGPreflightListenEventAccess() ? "true" : "false",
               IsSecureEventInputEnabled() ? "true" : "false");
        return 0;
    }
    if (!strcmp(argv[1], "counter")) { counters("counter"); return 0; }
    if (!strcmp(argv[1], "hold") && argc == 3) {
        EnableSecureEventInput();
        printf("hold secure_input_after_enable=%s\n", IsSecureEventInputEnabled() ? "true" : "false");
        sleep((unsigned)atoi(argv[2]));
        printf("hold secure_input_before_disable=%s\n", IsSecureEventInputEnabled() ? "true" : "false");
        DisableSecureEventInput();
        return 0;
    }
    if (!strcmp(argv[1], "post") && argc == 3) {
        int n = atoi(argv[2]);
        int ok = 0;
        for (int i = 0; i < n; i++) {
            CGEventRef d = CGEventCreateKeyboardEvent(NULL, (CGKeyCode)0, true);
            CGEventRef u = CGEventCreateKeyboardEvent(NULL, (CGKeyCode)0, false);
            if (d && u) { CGEventPost(kCGHIDEventTap, d); usleep(40000); CGEventPost(kCGHIDEventTap, u); usleep(40000); ok += 2; }
            if (d) CFRelease(d);
            if (u) CFRelease(u);
            CGEventRef fd = CGEventCreateKeyboardEvent(NULL, (CGKeyCode)56, true);
            CGEventRef fu = CGEventCreateKeyboardEvent(NULL, (CGKeyCode)56, false);
            if (fd && fu) {
                CGEventSetType(fd, kCGEventFlagsChanged); CGEventSetFlags(fd, kCGEventFlagMaskShift);
                CGEventSetType(fu, kCGEventFlagsChanged); CGEventSetFlags(fu, 0);
                CGEventPost(kCGHIDEventTap, fd); usleep(40000); CGEventPost(kCGHIDEventTap, fu); usleep(40000); ok += 2;
            }
            if (fd) CFRelease(fd);
            if (fu) CFRelease(fu);
        }
        printf("post events_posted=%d (key_down=%d key_up=%d flags_changed=%d)\n", ok, n, n, 2 * n);
        return 0;
    }
    if (!strcmp(argv[1], "tap") && argc == 3) {
        int secs = atoi(argv[2]);
        printf("tap listen_preflight=%s secure_at_start=%s\n", CGPreflightListenEventAccess() ? "true" : "false",
               IsSecureEventInputEnabled() ? "true" : "false");
        CGEventMask mask = CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventKeyUp) | CGEventMaskBit(kCGEventFlagsChanged);
        CFMachPortRef tap = CGEventTapCreate(kCGHIDEventTap, kCGHeadInsertEventTap, kCGEventTapOptionListenOnly, mask, cb, NULL);
        printf("tap created=%s\n", tap ? "true" : "false");
        if (!tap) return 1;
        CFRunLoopSourceRef src = CFMachPortCreateRunLoopSource(NULL, tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        printf("tap ready\n");
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, (CFTimeInterval)secs, false);
        printf("tap secure_at_end=%s enabled_at_end=%s\n", IsSecureEventInputEnabled() ? "true" : "false",
               CGEventTapIsEnabled(tap) ? "true" : "false");
        printf("tap seen_secure_off key_down=%ld key_up=%ld flags_changed=%ld\n", n_down[0], n_up[0], n_flags[0]);
        printf("tap seen_secure_on key_down=%ld key_up=%ld flags_changed=%ld\n", n_down[1], n_up[1], n_flags[1]);
        printf("tap disabled_events=%ld\n", n_disabled);
        return 0;
    }
    return 64;
}
