// Virtual pointer with absolute motion, for spike use only.
// Usage: vptr click X Y | vptr drag X1 Y1 X2 Y2 [STEPS]
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "vptr-client.h"

#define BTN_LEFT 0x110
#define EXTENT_X 1280
#define EXTENT_Y 800

struct ctx {
    struct wl_display *display;
    struct wl_seat *seat;
    struct zwlr_virtual_pointer_manager_v1 *manager;
    struct zwlr_virtual_pointer_v1 *pointer;
};

static void global(void *data, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version) {
    struct ctx *c = data;
    if (!strcmp(iface, wl_seat_interface.name)) c->seat = wl_registry_bind(r, name, &wl_seat_interface, version < 7 ? version : 7);
    else if (!strcmp(iface, zwlr_virtual_pointer_manager_v1_interface.name)) c->manager = wl_registry_bind(r, name, &zwlr_virtual_pointer_manager_v1_interface, version < 2 ? version : 2);
}
static void global_remove(void *data, struct wl_registry *r, uint32_t name) {}
static const struct wl_registry_listener registry = {global, global_remove};

static uint32_t now_ms(void) {
    struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

static void move_abs(struct ctx *c, int x, int y) {
    zwlr_virtual_pointer_v1_motion_absolute(c->pointer, now_ms(), x, y, EXTENT_X, EXTENT_Y);
    zwlr_virtual_pointer_v1_frame(c->pointer);
    wl_display_roundtrip(c->display);
    usleep(30000);
}

static void button(struct ctx *c, int pressed) {
    zwlr_virtual_pointer_v1_button(c->pointer, now_ms(), BTN_LEFT, pressed ? 1 : 0);
    zwlr_virtual_pointer_v1_frame(c->pointer);
    wl_display_roundtrip(c->display);
    usleep(30000);
}

int main(int argc, char **argv) {
    if (argc < 4) { fprintf(stderr, "usage: vptr click X Y | drag X1 Y1 X2 Y2 [steps]\n"); return 2; }
    struct ctx c = {0};
    c.display = wl_display_connect(NULL);
    if (!c.display) { fprintf(stderr, "vptr: cannot connect\n"); return 1; }
    struct wl_registry *r = wl_display_get_registry(c.display);
    wl_registry_add_listener(r, &registry, &c);
    wl_display_roundtrip(c.display);
    if (!c.manager) { fprintf(stderr, "vptr: no virtual pointer manager\n"); return 1; }
    c.pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(c.manager, c.seat);
    usleep(100000);

    int x1 = atoi(argv[2]), y1 = atoi(argv[3]);
    move_abs(&c, x1, y1);
    if (!strcmp(argv[1], "click")) {
        button(&c, 1); button(&c, 0);
    } else if (!strcmp(argv[1], "drag")) {
        if (argc < 6) { fprintf(stderr, "vptr: drag needs X2 Y2\n"); return 2; }
        int x2 = atoi(argv[4]), y2 = atoi(argv[5]);
        int steps = argc > 6 ? atoi(argv[6]) : 12;
        if (steps < 1) steps = 1;
        button(&c, 1);
        for (int i = 1; i <= steps; i++) {
            move_abs(&c, x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps);
        }
        button(&c, 0);
    } else { fprintf(stderr, "vptr: unknown command\n"); return 2; }

    usleep(100000);
    zwlr_virtual_pointer_v1_destroy(c.pointer);
    wl_display_roundtrip(c.display);
    wl_display_disconnect(c.display);
    return 0;
}
