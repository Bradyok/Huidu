# Huidu PX30 device trees (mainline Linux 6.18.53)

| File | Content |
|---|---|
| `px30-huidu.dtsi` | Common board: PMIC, rails, eMMC, GMAC, USB, UARTs, I2C, SPI0 FPGA loader, ADCs, WDT, GPU, VPU, VOPB + RGB pins |
| `px30-huidu-d15.dts` | D15, exact stock panel timing (known-good reference) |
| `px30-huidu-c15.dts` | C15, **D15 timing as placeholder** (`TODO capture from unit`) |

Source: stock D15 DTB, Rockchip BSP 4.4.159 (`products/BoxPlayer/v7.11.18.0/hardware/D15_rk-kernel.dts`).
There is no C-series DTB yet, so every value here comes from the D15.

Validated with the kernel's own flow (`make ARCH=arm64 rockchip/px30-huidu-{c15,d15}.dtb`), and also with
`cpp` + `dtc -@ -W no-unit_address_vs_reg`. Both give 0 errors. The default flags give 0 warnings.
With `W=1` or raw dtc there is 1 warning: `avoid_unnecessary_addr_size` on `/ethernet@ff360000/mdio`. It comes from
the PHY node that has no `reg` on purpose (see GMAC below). `dtbs_check` was not run because dtschema is not installed.

## Stock node → mainline node

| Stock (4.4 BSP) | Mainline | Notes |
|---|---|---|
| `chosen` (console=ttyFIQ0) + `fiq-debugger` (serial-id 2) | `&uart2` (uart2m0) + `chosen` ttyS2 | Mainline has no fiq-debugger; UART2 becomes a plain 8250-dw |
| `serial@ff158000` | `&uart1` | FPGA control link; stock pinctrl `uart1_xfer uart1_cts` kept |
| `serial@ff030000/ff168000/ff170000` (okay) | not enabled | EVB leftovers with no known user. uart0-cts collides with uart1-cts (GPIO1_C2) |
| `vccsys`, `vcc-phy-regulator` | `vcc5v0_sys`, `vcc_phy` | |
| `i2c0/pmic@20` rk809 | `&i2c0/rk809` | All 16 rails: names, min/max, always/boot-on and suspend states copied (DCDC4 and LDO4 are 3.3 V, not the EVB's 3.0 V) |
| `pmic-reset-func = 1`, sleep/power-off/reset pinctrl states | dropped | Mainline rk8xx has no rk809 reset-mode property and uses no slppin pinctrl states |
| `i2c1/rtc@51` | `&i2c1/pcf8563` | `rtc0` = PCF8563, `rtc1` = RK809 |
| `grf/io-domains`, `pmugrf/io-domains` | `&io_domains`, `&pmu_io_domains` | Same mapping. **Stock has no vccio6 (flash)**, and it is also omitted here |
| `dwmmc@ff390000` | `&emmc` | 8-bit, HS200 1.8 V, non-removable. Stock has no vmmc/vqmmc and no eMMC pwrseq |
| `dwmmc@ff370000/ff380000`, `nandc` | disabled (dtsi default) | SD/SDIO are off. The nandc "okay" was an EVB leftover |
| `ethernet@ff360000` | `&gmac` + `&mdio/ethernet-phy` | RMII, `clock_in_out = "input"`, SCLK_GMAC reparented to `gmac_clkin` (50 MHz), reset GPIO2_B5 active-low 0/50/50 ms |
| `usb2-phy@100`, `usb@ff300000/ff340000/ff350000` | `&u2phy*`, `&usb20_otg` (otg), `&usb_host0_{ehci,ohci}` | |
| `spi0/spi_cyclone4@00` (`huidu,spi_bus0_cs0`) | `&spi0/fpga@0` `altr,fpga-passive-serial` | See FPGA below |
| `watchdog@ff1e0000` | `&wdt` | |
| `tsadc`, `saradc` (vref vcc1v8_soc) | `&tsadc`, `&saradc` | Stock sets no tshut mode, so the driver default (CRU reset) applies |
| `gpu@ff400000` (mali-supply vdd_logic) | `&gpu` (Panfrost, mali-supply `vdd_log`) | Mainline OPP table replaces the stock PVTM one |
| `vpu_service`/`hevc_service`/`vpu_combo` | `&vpu`, `&vpu_mmu` (Hantro) | Mainline has no HEVC decoder node for PX30 |
| `display-subsystem` + `vop@ff460000` + `grf/rgb` + `panel` | `&display_subsystem`, `&vopb` (+pinctrl), `vopb_out/endpoint@2` → `panel` (`panel-dpi`) | See display below |
| `leds/sys_run` | `gpio-leds` `sys-run` (heartbeat, GPIO1_C5 active-low) | |
| `led-control` (`huidu,led-control`) | `gpio-leds` `led-red`/`led-green` (GPIO0_B4/B5, active-high) | Default off. Stock driver behaviour not traced |
| `pcie-control` (`huidu,pcie-control`) | `regulator-fixed` `vcc_4g` on GPIO0_A5, always-on | Stock probe drives the pin high ("pcie_pwren") |
| `key/test-key` (`rockchip,key`) | `gpio-keys` KEY_PROG1 on GPIO1_C4 | The stock flag is 0 (active-high), which is unusual for a button. Verify |
| `backlight` + `pwm0`, `rk809-sound`, `rk_headset`, `wireless-*`, `sdio-pwrseq`, OV5695/ISP/CIF, LVDS/DSI | dropped | EVB leftovers, not used by a panel-less FPGA link. Wi-Fi is USB |
| `rk_rga@ff480000` | none | **Mainline has no PX30 RGA node or driver** |
| `dmc`/`dfi`/`ddr_timing`, `cpu-boost`, `pvtm`, `cpuinfo`, `drm-logo` reserved-memory, `optee` | none | **No mainline PX30 devfreq/DMC.** DDR stays at the bootloader rate |

## Display: VOPB → internal RGB → panel-dpi

* Stock routes RGB from the big VOP: `display-subsystem/route/route-rgb` `connect` points to `vop@ff460000 endpoint@2`.
  The stock `grf/rgb` encoder has endpoints to both VOPs.
* Mainline has **no `rockchip,px30-rgb` node or compatible**. The RGB output is the VOP's internal encoder
  (`drivers/gpu/drm/rockchip/rockchip_rgb.c`, `CONFIG_ROCKCHIP_RGB`). Both `px30-vop-big` and `px30-vop-lit` set
  `VOP_FEATURE_INTERNAL_RGB`, and `vop_bind()` calls `rockchip_rgb_init()`. That function takes the first endpoint
  of the VOP `port` whose remote is **not** a Rockchip sub-driver (DSI and LVDS are disabled, so they are skipped) and
  attaches a panel or bridge there. So the supported topology is **VOP port endpoint → panel directly**. The LCDC pins
  must be claimed by the VOP node (`pinctrl-0` on `&vopb`; the same pattern is used in `rk3188-bqedison2qc.dts`).
* We use **VOPB** because mainline never programs the PX30 GRF RGB-source select (`GRF_PD_VO_CON1`; only the
  LVDS driver touches that register). At reset it selects VOPB, and that is also what stock used. VOPL stays disabled.
  If a bootloader switches that select to VOPL, mainline will not switch it back.
* Mainline limits you must verify on a scope or LA against a stock unit:
  * `rockchip_rgb` sets the output type to `LVDS`, so the VOP forces `rgb_dclk_pol = 1`. **`pixelclk-active` from the DT
    is ignored for the VOP output.**
  * The VOP never sets `DEN_NEGATIVE`, so **DE is always active-high**. The stock DT says `de-active = <0>` (active-low).
    If the stock VOP really drove DE low, the FPGA will need the opposite polarity. HSYNC/VSYNC positive is honoured.
  * Stock `bus-format = 0x1013` = `MEDIA_BUS_FMT_BGR888_1X24`. `panel-dpi` has no bus-format property, so mainline
    drives P888 (RGB888_1X24). **R and B may come out swapped**. If so, fix it in user space (XBGR8888 framebuffer)
    or with a panel-simple entry that carries BGR888.
  * The stock 20 ms prepare/enable/disable/unprepare delays cannot be expressed with `panel-dpi`.
  * DCLK_VOPB must reach exactly 12 MHz from the PLLs. Check `/sys/kernel/debug/clk/clk_summary`.

## FPGA loader (SPI0)

The stock driver (`drivers/char/cyclone4.c`, disassembled from the 4.4 `Image` at file offset ~0x3ef8xx) does this:
`of_get_named_gpio(np,"gpios",2)` → "io_conf_done", `(…,0)` → "io_nconfig", `(…,1)` → "io_nstatus". It then runs
`gpio_request` with the labels `cyclone4_nconfig/conf_done/nstatus`, drives **nCONFIG output high** and makes the other two inputs.
The result is **GPIO0_A0 = nCONFIG, GPIO0_A1 = nSTATUS, GPIO0_A2 = CONF_DONE**. Stock flags are 0 and the driver works on raw levels.
Mainline `altera-ps-spi` uses logical levels (the binding example uses nconfig/nstat active-low and confd active-high), so
`GPIOD_OUT_LOW` on the active-low nCONFIG gives the same idle-high pin as stock. **TODO verify on hardware.**
Open risk: Huidu's `fpga.img` is not a plain RBF (8-byte header, sync `CC 55 AA 33`) and the FPGA vendor is unknown.
`altera-ps-spi` may therefore not load it as-is (header, bit order, Cyclone timing). The fallback is spidev + GPIO in user space.
(Stock also had `sdio-pwrseq` on GPIO0_A2; SDIO is disabled so it never bound, and it is dropped here.)

## Unknowns / inferences

* All C15/C35/C36 values: timing, GPIOs, PHY, and the `/dev/mdio_gpio` + `/dev/audio_switch` hardware (not in the D15 DTB).
* PHY address and model: the stock DT has none, so the PHY node has no `reg` and relies on the of_mdio auto-scan. Set `reg` once known.
* The `vcc_phy` voltage and control are not described in stock.
* vccio6 (eMMC I/O) voltage: HS200 at 1.8 V is inherited from stock, and the rail is not declared.
* RK809 `clock-output-names` follow the mainline convention (`xin32k` on clkout2). Stock named them `rk808-clkout1/2`.
* `cpu-supply` is set on all four cores. Stock set it only on cpu0 (same rail).
* Memory: there is no `/memory` node. Stock U-Boot fills it in and must also reserve the TF-A/OP-TEE areas. Check `/proc/iomem`.
* Stock bootargs `root=PARTUUID=614e0000-0000 swiotlb=1` are not carried over. Rockchip U-Boot may append or override `bootargs`.
* The test-key polarity and the led-red/green polarity come from the stock flags only.
