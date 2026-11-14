// The stall fixture of tests/macos_stall_fixture.rs: one window with a plain
// field and a secure field, driven by lines on standard input. Every reply is
// one line of names and numbers; no reply holds the text of a field.
//
// Commands, each answered on standard output:
//   state            front=<0|1> key=<0|1> focus=<plain|secure|other> plain=<n> secure=<n>
//   plain | secure   moves the focus, with the insertion point after the field's
//                    text; answers focus=<field> at=<uptime ns>
//   clear            empties both fields and the stamps; answers cleared
//   which <c>...     answers which=<w>, one letter of <w> for each character
//                    <c>: p when only the plain field holds it, s when only
//                    the secure field holds it, b when both do, - when neither
//   where            answers x=<n> y=<n>: the centre of the secure field in
//                    display points, origin at the top left of the main display
//   stall <ms> [self]  answers waiting at=<ns>, sleeps on the main thread, then,
//                    with self, moves the focus to the secure field itself;
//                    answers woke at=<ns>
//   hide             hides the app; answers hidden
//   activate         asks to be the app in front; answers asked
//   away             yields the front to the Finder; answers away=<0|1>, 1 when
//                    the Finder runs
//   keys <ms> <k>... posts keys at the HID level from the thread that reads
//                    the commands, so also while the main thread waits; each
//                    <k> is a key code, pressed and let go, or a key code with
//                    d or u for its down or its up alone; <ms> lies between two
//                    of them; answers posted=<n> at=<ns>
//   click <x> <y>    posts a click of the left button there; answers clicked at=<ns>
//   autoleave <ms>   from now on the secure field gives the focus to the plain
//                    field by itself <ms> after it became first responder; -1
//                    stops it; answers autoleave=<ms>
//   stamps           answers one line: stamps n=<count> lost=<count>, then for
//                    each stamp kind:detail:ns:on, oldest first; ns on
//                    CLOCK_MONOTONIC_RAW, on the state of secure input read there
//   quit             ends the program
// Stamps are taken on the main thread alone: each command as it is handled
// (cmd_<verb>), the stall (waiting, woke, self_before, self_after), each event
// as the application dequeues it (ev, detail the event's type), the secure
// field becoming first responder (fr_secure), each change of a field's text
// (land_plain, land_secure, detail the length), each focus move
// (focus_before_<field>, focus_after_<field>), the leave by Return
// (left_before, left_after) and by autoleave (autoleave_before,
// autoleave_after). clear empties them.
// Return in the secure field moves the focus to the plain field and writes
// left_secure at=<ns>.
#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static NSWindow *window;
static NSTextField *plainField;
static NSSecureTextField *secureField;

static unsigned long long uptime_ns(void) {
    return clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
}

typedef struct {
    const char *kind;
    long detail;
    unsigned long long ns;
    int on;
} Stamp;

enum { STAMP_CAP = 16384 };
static Stamp stamps[STAMP_CAP];
static int stampCount;
static int stampLost;
static long autoleaveMs = -1;

static void stamp(const char *kind, long detail) {
    int on = IsSecureEventInputEnabled() ? 1 : 0;
    unsigned long long ns = clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW);
    if (stampCount >= STAMP_CAP) {
        stampLost++;
        return;
    }
    stamps[stampCount++] = (Stamp){kind, detail, ns, on};
}

static const char *commandKind(NSString *verb) {
    static const char *const known[] = {
        "state", "plain", "secure", "clear", "which", "where", "stall", "hide",
        "away", "activate", "autoleave", "stamps", "quit",
    };
    static const char *const kinds[] = {
        "cmd_state", "cmd_plain", "cmd_secure", "cmd_clear", "cmd_which", "cmd_where",
        "cmd_stall", "cmd_hide", "cmd_away", "cmd_activate", "cmd_autoleave", "cmd_stamps",
        "cmd_quit",
    };
    for (size_t i = 0; i < sizeof known / sizeof known[0]; i++) {
        if (strcmp(verb.UTF8String, known[i]) == 0) return kinds[i];
    }
    return "cmd_other";
}

static void say(NSString *line) {
    printf("%s\n", line.UTF8String);
    fflush(stdout);
}

static void post_key(CGKeyCode code, bool down) {
    CGEventRef event = CGEventCreateKeyboardEvent(NULL, code, down);
    if (event) {
        CGEventPost(kCGHIDEventTap, event);
        CFRelease(event);
    }
}

static void post_keys(NSArray<NSString *> *words) {
    useconds_t step = (useconds_t)words[1].intValue * 1000;
    int posted = 0;
    for (NSUInteger i = 2; i < words.count; i++) {
        NSString *word = words[i];
        CGKeyCode code = (CGKeyCode)word.intValue;
        if ([word hasSuffix:@"d"]) {
            post_key(code, true);
        } else if ([word hasSuffix:@"u"]) {
            post_key(code, false);
        } else {
            post_key(code, true);
            usleep(3000);
            post_key(code, false);
        }
        posted++;
        if (i + 1 < words.count) usleep(step);
    }
    say([NSString stringWithFormat:@"posted=%d at=%llu", posted, uptime_ns()]);
}

static void post_click(NSArray<NSString *> *words) {
    CGPoint at = CGPointMake(words[1].doubleValue, words[2].doubleValue);
    CGEventType types[2] = {kCGEventLeftMouseDown, kCGEventLeftMouseUp};
    for (int i = 0; i < 2; i++) {
        CGEventRef event = CGEventCreateMouseEvent(NULL, types[i], at, kCGMouseButtonLeft);
        if (event) {
            CGEventPost(kCGHIDEventTap, event);
            CFRelease(event);
        }
        usleep(5000);
    }
    say([NSString stringWithFormat:@"clicked at=%llu", uptime_ns()]);
}

static NSString *focusName(void) {
    NSResponder *first = window.firstResponder;
    if ([first isKindOfClass:[NSTextView class]]) {
        id delegate = ((NSTextView *)first).delegate;
        if (delegate == secureField) return @"secure";
        if (delegate == plainField) return @"plain";
    }
    if (first == secureField) return @"secure";
    if (first == plainField) return @"plain";
    return @"other";
}

static void focus(NSControl *field, NSString *name) {
    BOOL secure = field == secureField;
    stamp(secure ? "focus_before_secure" : "focus_before_plain", 0);
    [window makeFirstResponder:field];
    [field.currentEditor setSelectedRange:NSMakeRange(field.stringValue.length, 0)];
    stamp(secure ? "focus_after_secure" : "focus_after_plain", 0);
    say([NSString stringWithFormat:@"focus=%@ at=%llu", name, uptime_ns()]);
}

@interface Leaver : NSObject
- (void)left:(id)sender;
@end

@implementation Leaver
- (void)left:(id)sender {
    (void)sender;
    stamp("left_before", 0);
    [window makeFirstResponder:plainField];
    stamp("left_after", 0);
    say([NSString stringWithFormat:@"left_secure at=%llu", uptime_ns()]);
}
@end

@interface StallApp : NSApplication
@end

@implementation StallApp
- (void)sendEvent:(NSEvent *)event {
    stamp("ev", (long)event.type);
    [super sendEvent:event];
}
@end

@interface StallSecureField : NSSecureTextField
@end

@implementation StallSecureField
- (BOOL)becomeFirstResponder {
    BOOL became = [super becomeFirstResponder];
    stamp("fr_secure", became ? 1 : 0);
    if (became && autoleaveMs >= 0) {
        long after = autoleaveMs;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)after * (int64_t)NSEC_PER_MSEC),
                       dispatch_get_main_queue(), ^{
                           stamp("autoleave_before", after);
                           [window makeFirstResponder:plainField];
                           stamp("autoleave_after", after);
                       });
    }
    return became;
}
@end

@interface Lander : NSObject <NSTextFieldDelegate>
@end

@implementation Lander
- (void)controlTextDidChange:(NSNotification *)note {
    NSTextField *field = note.object;
    stamp(field == secureField ? "land_secure" : "land_plain", (long)field.stringValue.length);
}
@end

static void giveStamps(void) {
    NSMutableString *line = [NSMutableString stringWithFormat:@"stamps n=%d lost=%d", stampCount, stampLost];
    for (int i = 0; i < stampCount; i++) {
        [line appendFormat:@" %s:%ld:%llu:%d", stamps[i].kind, stamps[i].detail, stamps[i].ns,
                           stamps[i].on];
    }
    say(line);
}

static void handle(NSString *line) {
    NSArray<NSString *> *words = [line componentsSeparatedByString:@" "];
    NSString *verb = words.firstObject;
    if ([verb isEqualToString:@"clear"]) {
        stampCount = 0;
        stampLost = 0;
    }
    stamp(commandKind(verb), 0);
    if ([verb isEqualToString:@"state"]) {
        BOOL front = NSApp.isActive &&
            NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier == getpid();
        say([NSString stringWithFormat:@"front=%d key=%d focus=%@ plain=%lu secure=%lu",
             front ? 1 : 0, window.isKeyWindow ? 1 : 0, focusName(),
             (unsigned long)plainField.stringValue.length,
             (unsigned long)secureField.stringValue.length]);
    } else if ([verb isEqualToString:@"plain"]) {
        focus(plainField, @"plain");
    } else if ([verb isEqualToString:@"secure"]) {
        focus(secureField, @"secure");
    } else if ([verb isEqualToString:@"which"] && words.count >= 2) {
        NSMutableString *where = [NSMutableString string];
        NSString *plain = plainField.stringValue;
        NSString *secure = secureField.stringValue;
        for (NSUInteger i = 0; i < words[1].length; i++) {
            NSString *one = [words[1] substringWithRange:NSMakeRange(i, 1)];
            BOOL inPlain = [plain containsString:one];
            BOOL inSecure = [secure containsString:one];
            [where appendString:inPlain ? (inSecure ? @"b" : @"p") : (inSecure ? @"s" : @"-")];
        }
        say([NSString stringWithFormat:@"which=%@", where]);
    } else if ([verb isEqualToString:@"clear"]) {
        plainField.stringValue = @"";
        secureField.stringValue = @"";
        say(@"cleared");
    } else if ([verb isEqualToString:@"where"]) {
        NSRect inWindow = [secureField convertRect:secureField.bounds toView:nil];
        NSRect onScreen = [window convertRectToScreen:inWindow];
        CGFloat top = NSScreen.screens.firstObject.frame.size.height;
        say([NSString stringWithFormat:@"x=%.0f y=%.0f", NSMidX(onScreen),
             top - NSMidY(onScreen)]);
    } else if ([verb isEqualToString:@"stall"] && words.count >= 2) {
        int ms = words[1].intValue;
        BOOL itself = words.count >= 3 && [words[2] isEqualToString:@"self"];
        stamp("waiting", ms);
        say([NSString stringWithFormat:@"waiting at=%llu", uptime_ns()]);
        usleep((useconds_t)ms * 1000);
        stamp("woke", ms);
        if (itself) {
            stamp("self_before", 0);
            [window makeFirstResponder:secureField];
            stamp("self_after", 0);
        }
        say([NSString stringWithFormat:@"woke at=%llu", uptime_ns()]);
    } else if ([verb isEqualToString:@"hide"]) {
        [NSApp hide:nil];
        say(@"hidden");
    } else if ([verb isEqualToString:@"away"]) {
        NSRunningApplication *finder =
            [NSRunningApplication runningApplicationsWithBundleIdentifier:@"com.apple.finder"].firstObject;
        if (finder) {
            if (@available(macOS 14.0, *)) {
                [NSApp yieldActivationToApplication:finder];
            }
            [finder activateWithOptions:0];
        }
        say([NSString stringWithFormat:@"away=%d", finder ? 1 : 0]);
    } else if ([verb isEqualToString:@"activate"]) {
        [NSApp activateIgnoringOtherApps:YES];
        [window makeKeyAndOrderFront:nil];
        say(@"asked");
    } else if ([verb isEqualToString:@"autoleave"] && words.count >= 2) {
        autoleaveMs = words[1].intValue;
        say([NSString stringWithFormat:@"autoleave=%ld", autoleaveMs]);
    } else if ([verb isEqualToString:@"stamps"]) {
        giveStamps();
    } else if ([verb isEqualToString:@"quit"]) {
        exit(0);
    } else {
        say(@"unknown");
    }
}

int main(void) {
    @autoreleasepool {
        [StallApp sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        window = [[NSWindow alloc] initWithContentRect:NSMakeRect(200, 200, 420, 160)
                                             styleMask:NSWindowStyleMaskTitled
                                               backing:NSBackingStoreBuffered
                                                 defer:NO];
        plainField = [[NSTextField alloc] initWithFrame:NSMakeRect(20, 100, 380, 24)];
        secureField = [[StallSecureField alloc] initWithFrame:NSMakeRect(20, 40, 380, 24)];
        Lander *lander = [Lander new];
        plainField.delegate = lander;
        secureField.delegate = lander;
        Leaver *leaver = [Leaver new];
        secureField.target = leaver;
        secureField.action = @selector(left:);
        [window.contentView addSubview:plainField];
        [window.contentView addSubview:secureField];
        plainField.nextKeyView = secureField;
        secureField.nextKeyView = plainField;
        window.initialFirstResponder = plainField;
        [window makeKeyAndOrderFront:nil];
        [NSApp activateIgnoringOtherApps:YES];
        [NSThread detachNewThreadWithBlock:^{
            char buffer[256];
            while (fgets(buffer, sizeof buffer, stdin)) {
                buffer[strcspn(buffer, "\r\n")] = 0;
                NSString *line = [NSString stringWithUTF8String:buffer];
                NSArray<NSString *> *words = [line componentsSeparatedByString:@" "];
                if ([words.firstObject isEqualToString:@"keys"] && words.count >= 3) {
                    post_keys(words);
                } else if ([words.firstObject isEqualToString:@"click"] && words.count == 3) {
                    post_click(words);
                } else {
                    dispatch_async(dispatch_get_main_queue(), ^{ handle(line); });
                }
            }
            dispatch_async(dispatch_get_main_queue(), ^{ exit(0); });
        }];
        dispatch_async(dispatch_get_main_queue(), ^{
            [window makeFirstResponder:plainField];
            say([NSString stringWithFormat:@"ready pid=%d", getpid()]);
        });
        [NSApp run];
    }
    return 0;
}
