# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
"""Writes reviera_synthetic.epw: a synthetic, temperate-maritime weather year at 51.5° N, 0° E
for the sustainability samples. Every value is computed here (sun positions, a clear-sky model
dimmed by a deterministic day-to-day cloud cover, seasonal and daily temperature swings), so the
file is the project's own and can be shipped; it is not measured weather and must not be used to
design a real building.

    python3 make_weather.py   # rewrites reviera_synthetic.epw beside this script
"""

import math
import os

LAT, LON, TZ, ELEV = 51.5, 0.0, 0.0, 20.0
MONTH_DAYS = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]


def noise(i, seed):
    """A repeatable pseudo-random number in [0, 1)."""
    x = math.sin(i * 12.9898 + seed * 78.233) * 43758.5453
    return x - math.floor(x)


def smooth(day, seed):
    """Day-to-day noise in [0, 1), eased between days."""
    a, b = noise(int(day), seed), noise(int(day) + 1, seed)
    f = day - int(day)
    f = f * f * (3 - 2 * f)
    return a + (b - a) * f


def sun(doy, hour):
    """Sine of the solar altitude at the middle of the hour ending at `hour` (local time)."""
    decl = math.radians(23.45 * math.sin(math.radians(360 * (284 + doy) / 365)))
    b = math.radians(360 * (doy - 81) / 364)
    eot = 9.87 * math.sin(2 * b) - 7.53 * math.cos(b) - 1.5 * math.sin(b)
    solar = hour - 0.5 + (4 * (LON - 15 * TZ) + eot) / 60
    w = math.radians(15 * (solar - 12))
    phi = math.radians(LAT)
    return math.sin(phi) * math.sin(decl) + math.cos(phi) * math.cos(decl) * math.cos(w)


def main():
    lines = [
        f"LOCATION,Reviera Synthetic Temperate,,Synthetic,Reviera synthetic (not measured),000000,{LAT},{LON},{TZ},{ELEV}",
        "DESIGN CONDITIONS,0",
        "TYPICAL/EXTREME PERIODS,0",
        "GROUND TEMPERATURES,0",
        "HOLIDAYS/DAYLIGHT SAVINGS,No,0,0,0",
        "COMMENTS 1,Synthetic weather made by test-data/samples/make_weather.py for the BIM Reviera samples; not measured data",
        "COMMENTS 2,Clear-sky sun model dimmed by a repeatable cloud cover; GPL-3.0-or-later with the samples",
        "DATA PERIODS,1,1,Data,Monday, 1/ 1,12/31",
    ]
    n = 0
    for m, days in enumerate(MONTH_DAYS):
        for d in range(1, days + 1):
            doy = n // 24 + 1
            for h in range(1, 25):
                day = n / 24
                season = -math.cos(2 * math.pi * (day - 25) / 365)  # coldest late January
                cloud = min(1.0, max(0.0, 0.55 - 0.15 * season + 0.6 * (smooth(day / 2.5, 1) - 0.5)))
                s = sun(doy, h)
                i0 = 1367 * (1 + 0.033 * math.cos(math.radians(360 * doy / 365)))
                if s > 0.01:
                    air_mass = 1 / s
                    clear = i0 * 0.7 ** (air_mass ** 0.678)
                    dni = clear * (1 - cloud) ** 1.6
                    dhi = i0 * s * (0.07 + 0.22 * cloud)
                    ghi = dni * s + dhi
                    ext_h = i0 * s
                else:
                    dni = dhi = ghi = ext_h = 0.0
                daily = -math.cos(2 * math.pi * (h - 4) / 24)  # coolest before dawn, warmest mid-afternoon
                t = 10.8 + 7.2 * season + (2.0 + 3.5 * (1 - cloud)) * daily + 3.0 * (smooth(day / 3.0, 2) - 0.5)
                dew = t - (1.5 + 6.0 * (1 - cloud) * (0.5 + 0.5 * daily))
                rh = 100 * math.exp(17.625 * dew / (243.04 + dew)) / math.exp(17.625 * t / (243.04 + t))
                wind = 2.0 + 6.0 * smooth(day / 1.7, 3)
                wdir = int(360 * smooth(day / 4.0, 4)) % 360
                sky = round(10 * cloud)
                lines.append(
                    f"2001,{m + 1},{d},{h},60,A7A7,{t:.1f},{dew:.1f},{rh:.0f},101325,{ext_h:.0f},{i0:.0f},9999,"
                    f"{ghi:.0f},{dni:.0f},{dhi:.0f},999999,999999,999999,9999,{wdir},{wind:.1f},{sky},{sky},"
                    "9999,99999,9,999999999,999,0.120,0,88,0.200,0.0,0.0"
                )
                n += 1
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "reviera_synthetic.epw")
    with open(path, "w", newline="\r\n") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
