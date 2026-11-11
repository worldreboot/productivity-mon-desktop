// Native boundary for the Rust desktop client. Metadata only: never capture screens, browser contents or input.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <objc/runtime.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <math.h>

typedef struct { int32_t left, top, right, bottom; } PMonRect;
typedef struct { intptr_t identifier; PMonRect bounds; } PMonWindow;
_Static_assert(sizeof(PMonRect)==16,"PMonRect ABI");
_Static_assert(sizeof(PMonWindow)==24,"arm64 PMonWindow ABI");

static bool PMText(NSString *text, char *out, size_t capacity) {
    if (!text.length || !out || !capacity) return false;
    return [text getCString:out maxLength:capacity encoding:NSUTF8StringEncoding];
}
// One stable coordinate space across displays: Quartz top-left points times the primary display scale.
static CGFloat PMScale(void) { return NSScreen.screens.firstObject.backingScaleFactor ?: 1.0; }
static CGFloat PMTop(void) { return NSMaxY(NSScreen.screens.firstObject.frame); }
static PMonRect PMBounds(CGRect r) {
    CGFloat s=PMScale();
    return (PMonRect){(int32_t)llround(r.origin.x*s),(int32_t)llround(r.origin.y*s),
        (int32_t)llround(CGRectGetMaxX(r)*s),(int32_t)llround(CGRectGetMaxY(r)*s)};
}
static NSRunningApplication *PMFront(void) {
    NSRunningApplication *app=NSWorkspace.sharedWorkspace.frontmostApplication;
    return app.processIdentifier == NSProcessInfo.processInfo.processIdentifier ? nil : app;
}
bool pm_front_bundle(char *out,size_t capacity) { @autoreleasepool { return PMText(PMFront().bundleIdentifier,out,capacity); } }
bool pm_accessibility_trusted(void) { return AXIsProcessTrusted(); }
void pm_request_accessibility(void) {
    // This is called only from the user's "Enable website tracking" button, never at launch.
    NSDictionary *options=@{(__bridge NSString *)kAXTrustedCheckOptionPrompt:@YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}
bool pm_browser_title(char *out,size_t capacity) { @autoreleasepool {
    NSRunningApplication *app=PMFront();
    // Check the application again here so a focus change cannot expose another app's window title.
    NSArray *browsers=@[@"com.apple.safari",@"com.google.chrome",@"com.microsoft.edgemac",@"org.mozilla.firefox",@"com.brave.browser",@"com.operasoftware.opera",@"com.vivaldi.vivaldi",@"company.thebrowser.browser",@"app.zen-browser.zen",@"org.chromium.chromium"];
    if (!app || ![browsers containsObject:app.bundleIdentifier.lowercaseString] || !AXIsProcessTrusted()) return false;
    AXUIElementRef application=AXUIElementCreateApplication(app.processIdentifier);
    if (!application) return false;
    AXUIElementSetMessagingTimeout(application,0.15);
    CFTypeRef window=NULL,title=NULL;
    bool found=false;
    if (AXUIElementCopyAttributeValue(application,kAXFocusedWindowAttribute,&window)==kAXErrorSuccess && window && CFGetTypeID(window)==AXUIElementGetTypeID()) {
        AXUIElementSetMessagingTimeout((AXUIElementRef)window,0.15);
        if (AXUIElementCopyAttributeValue((AXUIElementRef)window,kAXTitleAttribute,&title)==kAXErrorSuccess && title && CFGetTypeID(title)==CFStringGetTypeID()) {
            found=PMText((__bridge NSString *)title,out,capacity);
        }
    }
    if (title) CFRelease(title);
    if (window) CFRelease(window);
    CFRelease(application);
    return found;
} }
void pm_local_time(int32_t *out) { @autoreleasepool {
    NSCalendar *calendar=[[NSCalendar alloc] initWithCalendarIdentifier:NSCalendarIdentifierGregorian];
    calendar.timeZone=NSTimeZone.localTimeZone;
    NSDateComponents *parts=[calendar components:NSCalendarUnitYear|NSCalendarUnitMonth|NSCalendarUnitDay|NSCalendarUnitHour fromDate:NSDate.date];
    out[0]=(int32_t)parts.year;out[1]=(int32_t)parts.month;out[2]=(int32_t)parts.day;out[3]=(int32_t)parts.hour;
} }
double pm_idle_seconds(void) { return CGEventSourceSecondsSinceLastEventType(kCGEventSourceStateCombinedSessionState,kCGAnyInputEventType); }
void pm_cursor(double *x,double *y) { @autoreleasepool {
    CGEventRef event=CGEventCreate(NULL);
    CGPoint point=event ? CGEventGetLocation(event) : CGPointZero;
    if (event) CFRelease(event);
    *x=point.x*PMScale();*y=point.y*PMScale();
} }
size_t pm_screens(PMonRect *out,size_t capacity) { @autoreleasepool {
    size_t count=0;
    for (NSScreen *screen in NSScreen.screens) {
        if (count==capacity) break;
        NSRect r=screen.visibleFrame;
        out[count++]=PMBounds(CGRectMake(r.origin.x,PMTop()-NSMaxY(r),r.size.width,r.size.height));
    }
    return count;
} }
static NSArray *PMonWindows(void) {
    // Bounds, order and owner PID are public window metadata; no window titles or image capture.
    return CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly|kCGWindowListExcludeDesktopElements,kCGNullWindowID));
}
size_t pm_windows(PMonWindow *out,size_t capacity) { @autoreleasepool {
    size_t count=0;
    for (NSDictionary *info in PMonWindows()) {
        if (count==capacity) break;
        if ([info[(__bridge NSString *)kCGWindowLayer] intValue]!=0 || [info[(__bridge NSString *)kCGWindowOwnerPID] intValue]==NSProcessInfo.processInfo.processIdentifier) continue;
        CGRect r;
        if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)info[(__bridge NSString *)kCGWindowBounds],&r) || r.size.width<120 || r.size.height<20) continue;
        out[count++]=(PMonWindow){[info[(__bridge NSString *)kCGWindowNumber] integerValue],PMBounds(r)};
    }
    return count;
} }
bool pm_busy(void) { @autoreleasepool {
    NSRunningApplication *app=PMFront();if (!app) return false;
    for (NSDictionary *info in PMonWindows()) {
        if ([info[(__bridge NSString *)kCGWindowOwnerPID] intValue]!=app.processIdentifier || [info[(__bridge NSString *)kCGWindowLayer] intValue]!=0) continue;
        CGRect r;
        if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)info[(__bridge NSString *)kCGWindowBounds],&r)) return false;
        for (NSScreen *screen in NSScreen.screens) {
            NSRect frame=screen.frame;
            CGRect full=CGRectMake(frame.origin.x,PMTop()-NSMaxY(frame),frame.size.width,frame.size.height);
            if (fabs(r.origin.x-full.origin.x)<2 && fabs(r.origin.y-full.origin.y)<2 && r.size.width>=full.size.width-2 && r.size.height>=full.size.height-2) return true;
        }
        return false;
    }
    return false;
} }
intptr_t pm_window_for_view(void *view) { return (intptr_t)(__bridge void *)((__bridge NSView *)view).window; }
bool pm_window_bounds(intptr_t handle,PMonRect *out) {
    NSWindow *window=(__bridge NSWindow *)(void *)handle;if (!window || !out) return false;
    NSRect r=window.frame;*out=PMBounds(CGRectMake(r.origin.x,PMTop()-NSMaxY(r),r.size.width,r.size.height));return true;
}
static BOOL PMCannotBecomeKey(id self,SEL selector) { (void)self;(void)selector;return NO; }
void pm_prepare_overlay(intptr_t handle) {
    NSWindow *window=(__bridge NSWindow *)(void *)handle;if (!window) return;
    // Winit's NSWindow subclass normally accepts focus, even without decorations. Override only our pet windows.
    Class overlay=objc_getClass("ProductivityMonNonactivatingWindow");
    if (!overlay) {
        overlay=objc_allocateClassPair(object_getClass(window),"ProductivityMonNonactivatingWindow",0);
        if (overlay) {
            class_addMethod(overlay,@selector(canBecomeKeyWindow),(IMP)PMCannotBecomeKey,"B@:");
            class_addMethod(overlay,@selector(canBecomeMainWindow),(IMP)PMCannotBecomeKey,"B@:");
            objc_registerClassPair(overlay);
        }
    }
    if (overlay) object_setClass(window,overlay);
    window.opaque=NO;window.backgroundColor=NSColor.clearColor;window.hasShadow=NO;window.hidesOnDeactivate=NO;
    window.collectionBehavior=NSWindowCollectionBehaviorCanJoinAllSpaces|NSWindowCollectionBehaviorFullScreenAuxiliary|NSWindowCollectionBehaviorIgnoresCycle;
}
void pm_move_window(intptr_t handle,int32_t x,int32_t y,bool on_top) {
    NSWindow *window=(__bridge NSWindow *)(void *)handle;if (!window) return;
    [window setFrameTopLeftPoint:NSMakePoint(x/PMScale(),PMTop()-y/PMScale())];
    if (on_top) [window orderFrontRegardless];
}
void pm_open_url(const char *url) { @autoreleasepool {
    NSURL *address=[NSURL URLWithString:[NSString stringWithUTF8String:url]];
    if (address && ([@"https" isEqualToString:address.scheme] || [@"http" isEqualToString:address.scheme])) [NSWorkspace.sharedWorkspace openURL:address];
} }
bool pm_copy_text(const char *text) { @autoreleasepool {
    NSPasteboard *pasteboard=NSPasteboard.generalPasteboard;[pasteboard clearContents];
    return [pasteboard setString:[NSString stringWithUTF8String:text] forType:NSPasteboardTypeString];
} }
static NSURL *PMApplicationURL(NSString *bundle) {
    for (NSRunningApplication *app in NSWorkspace.sharedWorkspace.runningApplications) {
        if (app.bundleIdentifier && [app.bundleIdentifier caseInsensitiveCompare:bundle]==NSOrderedSame && app.bundleURL) return app.bundleURL;
    }
    return [NSWorkspace.sharedWorkspace URLForApplicationWithBundleIdentifier:bundle];
}
bool pm_app_name(const char *bundle,char *out,size_t capacity) { @autoreleasepool {
    NSURL *url=PMApplicationURL([NSString stringWithUTF8String:bundle]);
    NSString *name;[url getResourceValue:&name forKey:NSURLLocalizedNameKey error:NULL];
    if ([name.pathExtension.lowercaseString isEqualToString:@"app"]) name=name.stringByDeletingPathExtension;
    return PMText(name,out,capacity);
} }
bool pm_app_icon(const char *bundle,uint8_t *pixels,uint32_t side) { @autoreleasepool {
    NSURL *url=PMApplicationURL([NSString stringWithUTF8String:bundle]);if (!url || !pixels || side!=32) return false;
    NSImage *image=[NSWorkspace.sharedWorkspace iconForFile:url.path];
    CGImageRef source=[image CGImageForProposedRect:NULL context:nil hints:nil];if (!source) return false;
    CGColorSpaceRef color=CGColorSpaceCreateDeviceRGB();
    CGContextRef context=CGBitmapContextCreate(pixels,side,side,8,side*4,color,kCGImageAlphaPremultipliedLast|kCGBitmapByteOrder32Big);
    CGColorSpaceRelease(color);if (!context) return false;
    CGContextDrawImage(context,CGRectMake(0,0,side,side),source);CGContextRelease(context);return true;
} }
void pm_play_wav(const uint8_t *bytes,size_t length,size_t index) { @autoreleasepool {
    static NSSound *sounds[6];if (index>=6) return;
    if (!sounds[index]) sounds[index]=[[NSSound alloc] initWithData:[NSData dataWithBytes:bytes length:length]];
    [sounds[index] play];
} }
