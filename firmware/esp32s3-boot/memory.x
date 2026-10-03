/* The loader's corner of the ESP32-S3's internal SRAM: the same addresses
   ESP-IDF's `bootloader.ld` uses, so every app linked for the C bootloader
   (esp-hal's RAM ends at 0x403CB700 / 0x3FCDB700) fits under this one.

   IRAM  0x403CB700 .. 0x403D2700   the loader's code (the C `iram_loader_seg`)
   DRAM  0x3FCE2700 .. 0x3FCE7700   its data (the C `dram_seg`)
   stack 0x3FCE7700 .. 0x3FCE9700   growing down from 0x3FCE9700, the C
                                     bootloader's "last usable address"
   The ROM's own data sits above 0x3FCE9700.

   The vectors need a 1 KB-aligned VECBASE, so they start at 0x403CB800 and
   the first 0x100 bytes of the region go unused. */
MEMORY
{
    vectors_seg ( RX ) : ORIGIN = 0x403CB800, LENGTH = 0x400
    iram_seg    ( RX ) : ORIGIN = 0x403CBC00, LENGTH = 0x6B00
    dram_seg    ( RW ) : ORIGIN = 0x3FCE2700, LENGTH = 0x5000
}

REGION_ALIAS("ROTEXT", iram_seg);
REGION_ALIAS("RWTEXT", iram_seg);
REGION_ALIAS("RODATA", dram_seg);
REGION_ALIAS("RWDATA", dram_seg);

_stack_start_cpu0 = 0x3FCE9700;
