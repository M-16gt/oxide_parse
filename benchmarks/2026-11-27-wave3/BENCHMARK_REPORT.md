# `oxide_parse` Full Benchmark Report — 2026-11-27

## Environment & Library Versions
- **OS / Platform:** `Windows-11-10.0.26200-SP0`
- **CPU / Cores:** `Intel64 Family 6 Model 190 Stepping 0, GenuineIntel` (4 logical cores)
- **Python:** `3.13.7 (tags/v3.13.7:bcee1c3, Aug 14 2025, 14:15:11) [MSC v.1944 64 bit (AMD64)]`
- **Run parameters:** `repeat=7`, `cooldown=2.0s`
- **Detected libraries:**
  - `bs4`: **4.15.0**
  - `crawl4ai`: **0.9.6**
  - `frostwork`: **0.2.2**
  - `html2text`: **2025.4.15**
  - `lxml`: **6.1.3**
  - `markdownify`: **1.2.4**
  - `oxide_parse`: **0.9.0**
  - `parsel`: **1.12.1**
  - `scrapling`: **0.5.1**
  - `selectolax`: **1.0.2**

## 1. Summary Table: Catalog Scaling (Medians, ms)
| Items | Noise | KiB | oxide_parse [scraper] | oxide_parse [lite] | oxide_parse [stream] | oxide_parse [xml] | selectolax [lexbor] | frostwork | lxml [direct] | parsel | scrapling | bs4 + lxml |
|---:|:---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | False | 24.6 | 3.035 | 1.211 | **0.713** | 1.641 | 8.004 | 1.421 | 31.987 | 41.108 | 76.586 | 101.629 |
| 100 | True | 61.0 | 11.287 | **1.265** | 3.078 | 1.419 | 11.155 | 2.329 | 28.267 | 70.569 | 83.985 | 299.053 |
| 1000 | False | 249.4 | 31.678 | 9.430 | **6.432** | 7.931 | 38.144 | 14.426 | 96.581 | 203.868 | 274.752 | 708.338 |
| 1000 | True | 613.7 | 84.296 | 20.243 | 17.863 | **16.095** | 101.907 | 24.228 | 208.702 | 446.967 | 539.037 | 1683.570 |
| 10000 | False | 2532.0 | 286.798 | 108.974 | **82.507** | 88.188 | 542.545 | 139.096 | 1031.640 | 2817.102 | 3983.414 | 6829.340 |
| 10000 | True | 6174.6 | 612.430 | 121.415 | 112.164 | **107.347** | 670.151 | 195.327 | 1398.381 | 2672.069 | 3689.635 | 12409.736 |

### `extract` vs `run_native` Comparison

Median deltas estimate Python object construction overhead. `run_native` also counts output values and frees the Rust `Value`, so negative deltas can appear on noisy runs.

| Items | Noise | Engine | extract (ms) | run_native (ms) | Delta (ms) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 2.668 | 2.713 | -0.045 |
| 100 | False | `lite` | 1.004 | 0.841 | 0.163 |
| 100 | False | `stream` | 1.341 | 0.944 | 0.397 |
| 100 | False | `xml` | 1.001 | 0.717 | 0.284 |
| 100 | True | `scraper` | 6.182 | 9.528 | -3.346 |
| 100 | True | `lite` | 1.392 | 1.742 | -0.349 |
| 100 | True | `stream` | 3.208 | 3.502 | -0.294 |
| 100 | True | `xml` | 1.468 | 1.524 | -0.057 |
| 1000 | False | `scraper` | 53.177 | 50.785 | 2.392 |
| 1000 | False | `lite` | 12.154 | 11.229 | 0.924 |
| 1000 | False | `stream` | 7.214 | 5.422 | 1.791 |
| 1000 | False | `xml` | 9.257 | 7.869 | 1.388 |
| 1000 | True | `scraper` | 74.868 | 72.978 | 1.891 |
| 1000 | True | `lite` | 16.175 | 14.521 | 1.654 |
| 1000 | True | `stream` | 12.653 | 12.545 | 0.109 |
| 1000 | True | `xml` | 16.174 | 12.926 | 3.248 |
| 10000 | False | `scraper` | 262.641 | 261.382 | 1.259 |
| 10000 | False | `lite` | 84.930 | 81.325 | 3.605 |
| 10000 | False | `stream` | 91.159 | 99.200 | -8.041 |
| 10000 | False | `xml` | 86.870 | 73.625 | 13.245 |
| 10000 | True | `scraper` | 654.307 | 616.319 | 37.987 |
| 10000 | True | `lite` | 168.371 | 151.020 | 17.352 |
| 10000 | True | `stream` | 244.546 | 138.895 | 105.651 |
| 10000 | True | `xml` | 141.034 | 137.191 | 3.843 |

## 2. Scenario Results
### Early-Exit Head + 1st Offer on 1591 KiB page
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.090** | 0.080 | 0.070 | 0.384 | x1.00 |
| `oxide_parse [xml]` | **11.160** | 8.977 | 7.280 | 27.974 | x124.42 |
| `oxide_parse [lite]` | **12.070** | 10.336 | 17.792 | 32.296 | x134.57 |
| `selectolax [lexbor]` | **21.584** | 18.234 | 3.002 | 31.333 | x240.63 |
| `oxide_parse [scraper]` | **93.841** | 74.707 | 21.233 | 99.017 | x1046.17 |
| `parsel` | **237.753** | 222.937 | 31.452 | 275.500 | x2650.53 |
| `lxml [direct]` | **264.343** | 196.030 | 77.217 | 445.193 | x2946.97 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **2.525** | 2.444 | 1.044 | 5.168 | x1.00 |
| `oxide_parse [lite]` | **2.842** | 2.669 | 0.276 | 44.343 | x1.13 |
| `oxide_parse [stream]` | **3.151** | 2.818 | 0.426 | 3.593 | x1.25 |
| `selectolax [lexbor]` | **4.527** | 3.932 | 4.585 | 9.150 | x1.79 |
| `oxide_parse [scraper]` | **12.529** | 11.556 | 2.105 | 16.206 | x4.96 |
| `parsel` | **45.231** | 43.029 | 13.774 | 90.165 | x17.91 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **8.270** | 6.654 | 2.085 | 10.123 | x1.00 |
| `oxide_parse [scraper]` | **17.805** | 16.599 | 4.147 | 27.380 | x2.15 |
| `lxml [xpath]` | **20.027** | 17.341 | 5.310 | 32.768 | x2.42 |
| `parsel [xpath]` | **26.264** | 23.317 | 3.745 | 32.941 | x3.18 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **2.378** | 2.251 | 2.337 | 6.176 | x1.00 |
| `selectolax + json.loads` | **2.546** | 2.319 | 0.827 | 6.630 | x1.07 |
| `oxide_parse [stream]` | **2.815** | 2.629 | 1.454 | 4.175 | x1.18 |
| `oxide_parse [lite]` | **2.942** | 2.580 | 0.623 | 4.217 | x1.24 |
| `oxide_parse [scraper]` | **4.674** | 3.855 | 4.281 | 9.964 | x1.97 |
| `lxml + json.loads` | **7.322** | 5.529 | 4.374 | 11.133 | x3.08 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **2.469** | 2.309 | 0.440 | 3.003 | x1.00 |
| `oxide_parse [stream]` | **2.775** | 2.545 | 0.363 | 3.483 | x1.12 |
| `oxide_parse [lite]` | **3.044** | 2.736 | 0.787 | 4.231 | x1.23 |
| `selectolax [strip_tags]` | **4.768** | 3.918 | 1.558 | 5.750 | x1.93 |
| `oxide_parse [scraper]` | **14.023** | 11.911 | 1.969 | 15.773 | x5.68 |
| `bs4 + lxml [decompose]` | **206.485** | 188.705 | 23.051 | 216.251 | x83.65 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.444** | 0.385 | 0.053 | 0.531 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **2.029** | 1.779 | 0.232 | 2.308 | x4.57 |
| `scrapling [SQLite memory] (v1 match)` | **8.049** | 6.032 | 7.551 | 15.121 | x18.12 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **3.730** | 3.116 | 3.768 | 15.908 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **4.844** | 4.498 | 5.489 | 17.423 | x1.30 |
| `scrapling [SQLite memory] (v2 relocate)` | **1087.197** | 1020.620 | 126.602 | 1239.699 | x291.47 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.390** | 1.337 | 0.180 | 4.016 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **9.846** | 7.413 | 3.613 | 14.313 | x7.08 |
| `scrapling [SQLite memory] (v1 match)` | **26.659** | 24.827 | 1.978 | 28.135 | x19.18 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **16.518** | 13.910 | 2.864 | 19.538 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **23.882** | 21.571 | 9.975 | 35.956 | x1.45 |
| `scrapling [SQLite memory] (v2 relocate)` | **8989.443** | 8588.478 | 822.352 | 10459.967 | x544.20 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **48.538** | 45.574 | 12.989 | 109.964 | x1.00 |
| `oxide_parse [xml] extract_many (Rayon)` | **53.708** | 43.334 | 27.663 | 74.947 | x1.11 |
| `oxide_parse [lite] extract_many (Rayon)` | **56.398** | 52.036 | 24.125 | 117.183 | x1.16 |
| `oxide_parse [xml] sequential loop` | **158.933** | 139.436 | 23.206 | 249.570 | x3.27 |
| `oxide_parse [stream] sequential loop` | **164.541** | 156.558 | 13.701 | 205.376 | x3.39 |
| `oxide_parse [lite] sequential loop` | **184.092** | 162.198 | 23.035 | 209.309 | x3.79 |
| `oxide_parse [scraper] extract_many (Rayon)` | **350.200** | 275.780 | 70.993 | 567.303 | x7.22 |
| `selectolax [lexbor] sequential loop` | **660.160** | 566.111 | 193.600 | 786.987 | x13.60 |
| `oxide_parse [scraper] sequential loop` | **991.586** | 962.206 | 269.852 | 1274.951 | x20.43 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `lite: fast ASCII ('$19.99')` | **4.636** | 4.501 | 1.652 | 8.684 | x1.00 |
| `xml: fast ASCII ('$19.99')` | **4.638** | 4.042 | 1.129 | 8.107 | x1.00 |
| `stream: fast ASCII ('$19.99')` | **4.741** | 3.560 | 2.674 | 9.133 | x1.02 |
| `xml: localized ('1 299,50 ₽')` | **5.950** | 5.283 | 1.194 | 8.355 | x1.28 |
| `stream: localized ('1 299,50 ₽')` | **6.003** | 4.982 | 2.770 | 8.427 | x1.30 |
| `lite: localized ('1 299,50 ₽')` | **6.814** | 5.952 | 1.293 | 9.598 | x1.47 |
| `scraper: fast ASCII ('$19.99')` | **15.021** | 13.511 | 3.167 | 21.280 | x3.24 |
| `scraper: localized ('1 299,50 ₽')` | **17.075** | 14.491 | 2.665 | 22.145 | x3.68 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `xml: 2 selectors (run_native)` | **6.319** | 5.810 | 0.485 | 8.610 | x1.00 |
| `stream: 2 selectors (run_native)` | **7.343** | 6.190 | 1.929 | 8.449 | x1.16 |
| `lite: 2 selectors (run_native)` | **7.959** | 6.721 | 4.477 | 13.995 | x1.26 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **11.479** | 10.040 | 2.549 | 15.169 | x1.82 |
| `xml: 11 selectors incl. 5 misses (run_native)` | **12.574** | 10.656 | 4.547 | 17.067 | x1.99 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **14.628** | 13.548 | 0.921 | 17.040 | x2.32 |
| `scraper: 2 selectors (run_native)` | **53.742** | 49.008 | 6.082 | 71.165 | x8.51 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **62.776** | 59.307 | 4.382 | 65.262 | x9.94 |

### HTML -> Markdown for RAG (300 blocks, 103.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **1.155** | 1.130 | 2.147 | 3.555 | x1.00 |
| `oxide_parse [lite]` | **1.306** | 1.189 | 0.174 | 1.633 | x1.13 |
| `oxide_parse [scraper]` | **6.045** | 5.484 | 2.310 | 8.665 | x5.24 |
| `html2text` | **121.419** | 114.940 | 18.858 | 147.629 | x105.17 |
| `markdownify` | **186.349** | 176.351 | 15.588 | 246.153 | x161.41 |

### Scrapling Bench: 5000 Nested Elements (408.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **5.699** | 4.820 | 1.739 | 11.363 | x1.00 |
| `oxide_parse [lite]` | **6.303** | 5.441 | 1.370 | 10.882 | x1.11 |
| `oxide_parse [xml]` | **6.426** | 5.712 | 1.215 | 9.095 | x1.13 |
| `frostwork` | **7.523** | 5.816 | 4.803 | 10.915 | x1.32 |
| `selectolax [lexbor]` | **12.440** | 10.270 | 2.699 | 15.565 | x2.18 |
| `oxide_parse [scraper]` | **35.057** | 31.855 | 6.908 | 44.706 | x6.15 |
| `lxml [direct]` | **54.623** | 52.425 | 28.240 | 85.410 | x9.59 |
| `parsel` | **157.094** | 143.720 | 39.351 | 332.801 | x27.57 |
| `scrapling` | **166.751** | 136.210 | 207.080 | 387.255 | x29.26 |
| `bs4 + lxml` | **624.941** | 563.346 | 88.960 | 762.326 | x109.66 |

### Scrapling Bench: find_similar vs adaptive list on 500 cards (52.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (adaptive list)` | **8.066** | 7.175 | 0.777 | 8.871 | x1.00 |
| `oxide_parse [scraper] (adaptive list)` | **10.520** | 10.036 | 1.334 | 12.324 | x1.30 |
| `scrapling [find_similar]` | **50.500** | 48.529 | 8.381 | 64.856 | x6.26 |

### Selectolax Bench: Strip script/style + body text + 800 links (83.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **1.167** | 1.139 | 0.096 | 6.092 | x1.00 |
| `oxide_parse [xml]` | **1.203** | 1.036 | 0.122 | 1.281 | x1.03 |
| `oxide_parse [lite]` | **1.490** | 1.208 | 0.595 | 2.225 | x1.28 |
| `selectolax [lexbor]` | **2.170** | 1.919 | 0.292 | 3.556 | x1.86 |
| `oxide_parse [scraper]` | **6.922** | 4.787 | 3.367 | 9.498 | x5.93 |
| `lxml [direct]` | **14.696** | 12.019 | 4.582 | 17.662 | x12.60 |
| `parsel` | **18.767** | 16.901 | 2.453 | 20.483 | x16.09 |
| `bs4 + lxml` | **109.601** | 96.664 | 16.110 | 140.582 | x93.95 |

### Frostwork Bench: Grouped Page (header + 2000 offers, 334.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **9.358** | 7.570 | 3.167 | 13.440 | x1.00 |
| `oxide_parse [xml]` | **10.787** | 9.241 | 4.804 | 17.670 | x1.15 |
| `oxide_parse [lite]` | **12.123** | 10.072 | 2.377 | 16.012 | x1.30 |
| `frostwork` | **17.284** | 16.505 | 1.446 | 19.053 | x1.85 |
| `oxide_parse [scraper]` | **38.566** | 34.346 | 7.423 | 52.297 | x4.12 |
| `selectolax [lexbor]` | **65.918** | 61.650 | 9.552 | 79.075 | x7.04 |
| `parsel` | **245.472** | 221.176 | 26.889 | 275.161 | x26.23 |
| `scrapling` | **359.363** | 331.442 | 29.945 | 380.771 | x38.40 |

### Frostwork Bench: first_only Early Exit (1591 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.092** | 0.078 | 0.034 | 0.123 | x1.00 |
| `frostwork [first_only]` | **0.313** | 0.270 | 0.136 | 1.105 | x3.40 |
| `oxide_parse [xml]` | **7.311** | 6.927 | 3.117 | 11.082 | x79.38 |
| `oxide_parse [lite]` | **11.855** | 9.572 | 3.438 | 33.304 | x128.71 |
| `selectolax [lexbor]` | **20.713** | 18.933 | 6.167 | 26.306 | x224.90 |
| `oxide_parse [scraper]` | **73.288** | 66.964 | 9.467 | 104.043 | x795.74 |
| `parsel` | **252.745** | 227.025 | 31.404 | 283.520 | x2744.24 |

### Parsel/Scrapy Quotes Bench (500 quotes with nested tags, 153.0 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.623** | 4.076 | 4.114 | 8.394 | x1.00 |
| `frostwork` | **7.570** | 4.973 | 1.427 | 11.092 | x1.64 |
| `oxide_parse [scraper]` | **15.047** | 12.857 | 3.341 | 20.437 | x3.25 |
| `selectolax [lexbor]` | **23.977** | 20.383 | 2.943 | 30.367 | x5.19 |
| `lxml [direct]` | **57.851** | 51.036 | 11.233 | 68.644 | x12.51 |
| `parsel` | **90.655** | 78.409 | 10.201 | 97.155 | x19.61 |
| `scrapling` | **139.363** | 120.905 | 13.746 | 154.668 | x30.14 |
| `bs4 + lxml` | **320.411** | 277.521 | 76.824 | 363.395 | x69.30 |

### Crawl4AI Nested E-commerce Catalog (30x4 products, 92.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.981** | 4.483 | 1.057 | 6.958 | x1.00 |
| `oxide_parse [scraper]` | **11.916** | 10.239 | 1.561 | 12.780 | x2.39 |
| `selectolax [lexbor]` | **21.847** | 20.307 | 2.842 | 26.539 | x4.39 |
| `parsel` | **88.548** | 82.032 | 15.363 | 109.781 | x17.78 |
| `crawl4ai [JsonXPath]` | **159.028** | 131.943 | 27.471 | 175.209 | x31.93 |
| `crawl4ai [JsonCss]` | **340.036** | 311.585 | 33.896 | 360.729 | x68.27 |


## Skipped Implementations

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

One warmup run, randomized order per round, GC run before timing, and all outputs verified. Cooldown pauses run between scenario groups. Sizes are in UTF-8 bytes.

OS background load, thermals, and CPU clocks were not pinned. `--quick` checks functionality on smaller catalogs.

## Comparison with the previous full run

Both runs used the same benchmark source hashes and dependency versions. The 10,000-product fixtures used 2,592,805 bytes without noise and 6,322,805 bytes with noise in both runs. The previous measurements are from commit `614a8ba`.

| Implementation | Without noise, previous → current (ms) | With noise, previous → current (ms) |
| --- | ---: | ---: |
| oxide_parse stream | 53.3 → 82.5 | 80.6 → 112.2 |
| oxide_parse lite | 69.3 → 109.0 | 104.5 → 121.4 |
| selectolax lexbor | 352.1 → 542.5 | 545.2 → 670.2 |
| parsel | 1563.5 → 2817.1 | 2189.1 → 2672.1 |

The runs took place in separate sessions. Several competitors also slowed down. The measurements establish a slowdown between sessions, and attributing it to the code would require alternating builds under matching conditions.

## New extraction APIs

Each synthetic workload was measured 15 times and its output was verified.

| Workload | Median (ms) | IQR (ms) |
| --- | ---: | ---: |
| Extract 1,000 JSONL records | 5.630 | 1.693 |
| Extract 1,000 CSV records | 17.313 | 2.157 |
| Render 150 paragraphs as Markdown with automatic content detection | 0.523 | 0.299 |
| Render the same paragraphs with an explicit article selector | 0.362 | 0.402 |
| Resolve 500 JSON-LD references across 500 products | 10.390 | 5.116 |
| Traverse 100 pages breadth-first with an in-memory fetcher | 12.273 | 2.421 |

The full suite covered 15 benchmark groups and 26 result sections. All 229 timing series passed output validation. The tested release wheel included all five engines. The Python suite reported 1,380 passes, 111 skips, and 13 expected failures. The Rust suite reported 116 passes, including doctests.
