/*
 * Spike K5 guest. It draws a moving square and a frame counter bar as fast
 * as it can, so every screendump can differ. Every 250 ms of guest time it
 * prints how many frames it drew, so the host can see the guest slow down:
 *   @RC stage=k5 ev=rate it=<frames drawn> ms=<guest ms since start>
 */
#include <stdint.h>

#include <robco/boot.h>
#include <robco/fb.h>
#include <robco/klibc.h>
#include <robco/timer.h>
#include <robco/uart.h>

#define SQ 32u

void robco_main(void)
{
    uint32_t it = 0, x = 0, y = 0, start, next;
    uint32_t bg, fg, bar;

    uart_init(115200);
    if (fb_init(K5_W, K5_H, 32) != 0) {
        k_printf("@RC stage=k5 ev=error what=fb_init\n");
        robco_halt();
    }
    bg = fb_rgb(0, 0, 0);
    fg = fb_rgb(0x33, 0xff, 0x66);
    bar = fb_rgb(0xff, 0xb0, 0x00);
    fb_clear(bg);
    k_printf("@RC stage=k5 ev=ready w=%u h=%u\n", (unsigned)K5_W, (unsigned)K5_H);
    start = timer_now_ms();
    next = start + 250;
    for (;;) {
        fb_fill_rect(x, y, SQ, SQ, bg);
        x = (x + 3) % (K5_W - SQ);
        y = (y + 2) % (K5_H - SQ - 8);
        fb_fill_rect(x, y, SQ, SQ, fg);
        /* The low 32 bits of the counter as a row of 32 cells at the bottom. */
        for (uint32_t b = 0; b < 32; b++) {
            fb_fill_rect(b * 8, K5_H - 8, 7, 8, (it >> b) & 1 ? bar : bg);
        }
        it++;
        if ((int32_t)(timer_now_ms() - next) >= 0) {
            k_printf("@RC stage=k5 ev=rate it=%u ms=%u\n", (unsigned)it,
                     (unsigned)(timer_now_ms() - start));
            next += 250;
        }
    }
}
