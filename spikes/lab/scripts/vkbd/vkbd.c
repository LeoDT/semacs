// Virtual keyboard with a real XKB keymap, for spike use only.
//
// `wtype` uploads a synthetic keymap whose keys only have one level, so Shift
// and dead keys cannot be observed through it. This tool compiles the same
// evdev keymap the compositor uses and sends keycodes for named keysyms, which
// makes Shift / AltGr / dead keys behave like physical input.
//
// Usage: vkbd [--layout us] [--variant intl] "ctrl+shift+question" "a" ...
//   modifiers: ctrl shift alt super(logo) altgr
//   keys: xkb keysym names (a, question, dead_acute, ...)
// All keys in one invocation share one virtual keyboard (and one compose
// sequence); batches that must compose have to stay in the same invocation.
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mman.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include "virtual-keyboard-client.h"

struct ctx {
    struct wl_display *display;
    struct wl_seat *seat;
    struct zwp_virtual_keyboard_manager_v1 *manager;
    struct zwp_virtual_keyboard_v1 *keyboard;
    struct xkb_context *xkb_ctx;
    struct xkb_keymap *keymap;
    unsigned depressed;
};

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    struct ctx *c = data;
    if (!strcmp(interface, wl_seat_interface.name)) {
        c->seat = wl_registry_bind(registry, name, &wl_seat_interface, version < 7 ? version : 7);
    } else if (!strcmp(interface, zwp_virtual_keyboard_manager_v1_interface.name)) {
        c->manager = wl_registry_bind(registry, name, &zwp_virtual_keyboard_manager_v1_interface, 1);
    }
}
static void registry_remove(void *data, struct wl_registry *registry, uint32_t name) {}
static const struct wl_registry_listener registry_listener = {registry_global, registry_remove};

static void die(const char *msg) { fprintf(stderr, "vkbd: %s\n", msg); exit(1); }

static void sync_mods(struct ctx *c) {
    zwp_virtual_keyboard_v1_modifiers(c->keyboard, c->depressed, 0, 0, 0);
    wl_display_roundtrip(c->display);
}

static int mod_index(struct ctx *c, const char *name) {
    int idx = xkb_keymap_mod_get_index(c->keymap, name);
    if (idx == XKB_MOD_INVALID) return -1;
    return idx;
}

static xkb_keysym_t mod_key_for_index(struct ctx *c, int idx) {
    const char *name = xkb_keymap_mod_get_name(c->keymap, idx);
    if (!name) return XKB_KEY_NoSymbol;
    if (!strcmp(name, "Shift")) return XKB_KEY_Shift_L;
    if (!strcmp(name, "Lock")) return XKB_KEY_Caps_Lock;
    if (!strcmp(name, "Control")) return XKB_KEY_Control_L;
    if (!strcmp(name, "Mod1")) return XKB_KEY_Alt_L;
    if (!strcmp(name, "Mod2")) return XKB_KEY_Num_Lock;
    if (!strcmp(name, "Mod4")) return XKB_KEY_Super_L;
    if (!strcmp(name, "Mod5")) return XKB_KEY_ISO_Level3_Shift;
    return XKB_KEY_NoSymbol;
}

static int find_key(struct ctx *c, xkb_keysym_t target, xkb_mod_mask_t *level_mods) {
    xkb_keycode_t min = xkb_keymap_min_keycode(c->keymap);
    xkb_keycode_t max = xkb_keymap_max_keycode(c->keymap);
    for (xkb_keycode_t kc = min; kc <= max; kc++) {
        int levels = xkb_keymap_num_levels_for_key(c->keymap, kc, 0);
        for (int level = 0; level < levels; level++) {
            const xkb_keysym_t *syms = NULL;
            int n = xkb_keymap_key_get_syms_by_level(c->keymap, kc, 0, level, &syms);
            for (int i = 0; i < n; i++) {
                if (syms[i] == target) {
                    xkb_mod_mask_t masks[8];
                    int m = xkb_keymap_key_get_mods_for_level(c->keymap, kc, 0, level, masks, 8);
                    xkb_mod_mask_t best = 0;
                    int best_pop = 99;
                    int lock = xkb_keymap_mod_get_index(c->keymap, "Lock");
                    for (int k = 0; k < m; k++) {
                        if (lock != XKB_MOD_INVALID && (masks[k] & (1u << lock))) continue;
                        int pop = __builtin_popcount(masks[k]);
                        if (pop < best_pop) { best_pop = pop; best = masks[k]; }
                    }
                    *level_mods = best;
                    return (int)kc;
                }
            }
        }
    }
    return -1;
}

static int keycode_for_keysym(struct ctx *c, xkb_keysym_t sym, xkb_mod_mask_t *mods) {
    int kc = find_key(c, sym, mods);
    if (kc < 0) {
        char buf[64] = {0};
        xkb_keysym_get_name(sym, buf, sizeof buf);
        fprintf(stderr, "vkbd: keysym %s not in keymap\n", buf);
        exit(1);
    }
    return kc;
}

static void send_key(struct ctx *c, int keycode, int pressed) {
    /* xkb keycodes are evdev scancodes + 8; the protocol speaks evdev. */
    zwp_virtual_keyboard_v1_key(c->keyboard, 0, (uint32_t)(keycode - 8),
                                pressed ? WL_KEYBOARD_KEY_STATE_PRESSED : WL_KEYBOARD_KEY_STATE_RELEASED);
    wl_display_roundtrip(c->display);
}

int main(int argc, char **argv) {
    const char *layout = "us", *variant = "";
    int argi = 1;
    while (argi < argc && argv[argi][0] == '-') {
        if (!strcmp(argv[argi], "--layout") && argi + 1 < argc) { layout = argv[++argi]; }
        else if (!strcmp(argv[argi], "--variant") && argi + 1 < argc) { variant = argv[++argi]; }
        else { fprintf(stderr, "vkbd: bad option %s\n", argv[argi]); return 2; }
        argi++;
    }
    if (argi >= argc) { fprintf(stderr, "vkbd: no keys given\n"); return 2; }

    struct ctx c = {0};
    c.display = wl_display_connect(NULL);
    if (!c.display) die("cannot connect to wayland display");
    struct wl_registry *registry = wl_display_get_registry(c.display);
    wl_registry_add_listener(registry, &registry_listener, &c);
    wl_display_roundtrip(c.display);
    if (!c.seat || !c.manager) die("compositor lacks wl_seat or virtual-keyboard protocol");

    c.xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_rule_names names = {.rules = "evdev", .model = "pc105", .layout = layout, .variant = variant, .options = ""};
    c.keymap = xkb_keymap_new_from_names(c.xkb_ctx, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    if (!c.keymap) die("cannot compile xkb keymap");
    char *text = xkb_keymap_get_as_string(c.keymap, XKB_KEYMAP_FORMAT_TEXT_V1);
    if (!text) die("cannot serialize keymap");
    size_t len = strlen(text) + 1;
    int fd = memfd_create("vkbd-keymap", MFD_CLOEXEC);
    if (fd < 0) die("memfd_create failed");
    if (write(fd, text, len) != (ssize_t)len) die("write keymap failed");
    lseek(fd, 0, SEEK_SET);
    free(text);

    c.keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(c.manager, c.seat);
    zwp_virtual_keyboard_v1_keymap(c.keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, len);
    wl_display_roundtrip(c.display);
    close(fd);
    usleep(100000);

    int shift_idx = mod_index(&c, "Shift");
    int ctrl_idx = mod_index(&c, "Control");
    int alt_idx = mod_index(&c, "Mod1");
    int logo_idx = mod_index(&c, "Mod4");
    int lv3_idx = mod_index(&c, "Mod5");

    for (int i = argi; i < argc; i++) {
        unsigned extra = 0;
        char *spec = strdup(argv[i]);
        char *key_name = strrchr(spec, '+');
        if (key_name) {
            *key_name++ = '\0';
            char *save = NULL;
            for (char *tok = strtok_r(spec, "+", &save); tok; tok = strtok_r(NULL, "+", &save)) {
                int idx;
                if (!strcmp(tok, "ctrl")) idx = ctrl_idx;
                else if (!strcmp(tok, "shift")) idx = shift_idx;
                else if (!strcmp(tok, "alt")) idx = alt_idx;
                else if (!strcmp(tok, "super") || !strcmp(tok, "logo")) idx = logo_idx;
                else if (!strcmp(tok, "altgr")) idx = lv3_idx;
                else { fprintf(stderr, "vkbd: bad modifier %s\n", tok); return 2; }
                if (idx == XKB_MOD_INVALID) { fprintf(stderr, "vkbd: modifier %s missing from keymap\n", tok); return 2; }
                extra |= 1u << idx;
            }
        } else {
            key_name = spec;
        }

        xkb_keysym_t sym = xkb_keysym_from_name(key_name, XKB_KEYSYM_NO_FLAGS);
        if (sym == XKB_KEY_NoSymbol) { fprintf(stderr, "vkbd: bad keysym %s\n", key_name); return 2; }
        xkb_mod_mask_t level_mods = 0;
        int kc = keycode_for_keysym(&c, sym, &level_mods);
        unsigned wanted = extra | level_mods;
        unsigned newly = wanted & ~c.depressed;

        for (int idx = 0; idx < 32; idx++) {
            if (!(newly & (1u << idx))) continue;
            xkb_keysym_t msym = mod_key_for_index(&c, idx);
            if (msym == XKB_KEY_NoSymbol) continue;
            xkb_mod_mask_t dummy = 0;
            int mkc = keycode_for_keysym(&c, msym, &dummy);
            send_key(&c, mkc, 1);
            c.depressed |= 1u << idx;
        }
        sync_mods(&c);
        usleep(20000);
        send_key(&c, kc, 1);
        usleep(20000);
        send_key(&c, kc, 0);
        usleep(20000);
        for (int idx = 0; idx < 32; idx++) {
            if (!(newly & (1u << idx))) continue;
            xkb_keysym_t msym = mod_key_for_index(&c, idx);
            if (msym == XKB_KEY_NoSymbol) continue;
            xkb_mod_mask_t dummy = 0;
            int mkc = keycode_for_keysym(&c, msym, &dummy);
            send_key(&c, mkc, 0);
            c.depressed &= ~(1u << idx);
        }
        sync_mods(&c);
        usleep(30000);
        free(spec);
    }

    usleep(100000);
    zwp_virtual_keyboard_v1_destroy(c.keyboard);
    wl_display_roundtrip(c.display);
    wl_display_disconnect(c.display);
    return 0;
}
