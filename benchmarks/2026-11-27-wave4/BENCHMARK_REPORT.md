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
| 100 | False | 24.6 | 1.690 | 0.649 | **0.544** | 0.566 | 2.792 | 0.886 | 5.977 | 18.233 | 24.414 | 39.034 |
| 100 | True | 61.0 | 4.039 | 0.774 | 0.981 | **0.748** | 4.306 | 1.206 | 9.005 | 17.172 | 23.889 | 77.468 |
| 1000 | False | 249.4 | 18.290 | 5.863 | **5.240** | 5.396 | 27.097 | 8.046 | 65.211 | 200.001 | 217.269 | 392.896 |
| 1000 | True | 613.7 | 72.502 | 12.929 | 11.598 | **10.883** | 50.187 | 18.559 | 149.877 | 301.385 | 321.333 | 962.336 |
| 10000 | False | 2532.0 | 160.637 | 51.700 | **46.384** | 51.964 | 314.976 | 74.527 | 580.305 | 1400.595 | 2104.310 | 3873.092 |
| 10000 | True | 6174.6 | 467.401 | 94.509 | 87.436 | **78.209** | 507.183 | 133.557 | 1306.318 | 1986.038 | 2908.151 | 9493.349 |

### `extract` vs `run_native` Comparison

Median deltas estimate Python object construction overhead. `run_native` also counts output values and frees the Rust `Value`, so negative deltas can appear on noisy runs.

| Items | Noise | Engine | extract (ms) | run_native (ms) | Delta (ms) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.732 | 1.284 | 0.448 |
| 100 | False | `lite` | 0.513 | 0.436 | 0.077 |
| 100 | False | `stream` | 0.462 | 0.422 | 0.039 |
| 100 | False | `xml` | 0.459 | 0.497 | -0.038 |
| 100 | True | `scraper` | 3.167 | 3.203 | -0.036 |
| 100 | True | `lite` | 0.725 | 0.703 | 0.023 |
| 100 | True | `stream` | 0.697 | 0.630 | 0.067 |
| 100 | True | `xml` | 0.643 | 0.661 | -0.019 |
| 1000 | False | `scraper` | 15.171 | 15.056 | 0.115 |
| 1000 | False | `lite` | 4.583 | 4.412 | 0.171 |
| 1000 | False | `stream` | 4.284 | 4.198 | 0.086 |
| 1000 | False | `xml` | 4.737 | 4.001 | 0.737 |
| 1000 | True | `scraper` | 42.322 | 38.644 | 3.678 |
| 1000 | True | `lite` | 8.102 | 7.492 | 0.610 |
| 1000 | True | `stream` | 6.681 | 7.939 | -1.258 |
| 1000 | True | `xml` | 8.965 | 8.254 | 0.710 |
| 10000 | False | `scraper` | 166.276 | 178.459 | -12.183 |
| 10000 | False | `lite` | 52.292 | 45.325 | 6.967 |
| 10000 | False | `stream` | 46.151 | 42.647 | 3.504 |
| 10000 | False | `xml` | 62.204 | 64.963 | -2.759 |
| 10000 | True | `scraper` | 390.353 | 390.614 | -0.261 |
| 10000 | True | `lite` | 97.456 | 113.976 | -16.520 |
| 10000 | True | `stream` | 130.587 | 163.349 | -32.763 |
| 10000 | True | `xml` | 80.146 | 73.693 | 6.453 |

## 2. Scenario Results
### Early-Exit Head + 1st Offer on 1591 KiB page
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.075** | 0.065 | 0.006 | 0.085 | x1.00 |
| `oxide_parse [xml]` | **6.042** | 4.555 | 2.805 | 9.673 | x80.99 |
| `oxide_parse [lite]` | **7.624** | 6.223 | 1.928 | 12.207 | x102.20 |
| `selectolax [lexbor]` | **14.560** | 12.622 | 6.577 | 22.429 | x195.17 |
| `oxide_parse [scraper]` | **45.630** | 43.096 | 22.465 | 97.337 | x611.66 |
| `lxml [direct]` | **131.621** | 123.327 | 26.495 | 191.836 | x1764.36 |
| `parsel` | **162.162** | 145.455 | 56.160 | 230.097 | x2173.75 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **2.241** | 2.066 | 0.805 | 4.401 | x1.00 |
| `oxide_parse [lite]` | **2.417** | 2.046 | 0.303 | 3.038 | x1.08 |
| `oxide_parse [stream]` | **2.515** | 2.082 | 0.432 | 3.396 | x1.12 |
| `selectolax [lexbor]` | **4.258** | 2.593 | 1.605 | 5.021 | x1.90 |
| `oxide_parse [scraper]` | **9.646** | 8.909 | 0.671 | 13.008 | x4.30 |
| `parsel` | **32.936** | 30.794 | 3.220 | 39.321 | x14.69 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **5.821** | 4.803 | 1.370 | 8.100 | x1.00 |
| `lxml [xpath]` | **11.107** | 9.709 | 1.903 | 12.853 | x1.91 |
| `oxide_parse [scraper]` | **11.821** | 9.005 | 4.348 | 16.448 | x2.03 |
| `parsel [xpath]` | **14.426** | 12.393 | 3.194 | 19.318 | x2.48 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **1.582** | 1.540 | 0.215 | 2.078 | x1.00 |
| `oxide_parse [lite]` | **1.980** | 1.668 | 0.371 | 2.091 | x1.25 |
| `oxide_parse [xml]` | **2.204** | 1.939 | 0.352 | 2.564 | x1.39 |
| `oxide_parse [stream]` | **2.319** | 1.800 | 0.574 | 3.541 | x1.47 |
| `lxml + json.loads` | **3.730** | 3.563 | 0.334 | 6.666 | x2.36 |
| `oxide_parse [scraper]` | **4.037** | 2.714 | 3.179 | 7.151 | x2.55 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **1.656** | 1.617 | 0.984 | 3.616 | x1.00 |
| `oxide_parse [stream]` | **1.747** | 1.713 | 0.291 | 2.421 | x1.06 |
| `oxide_parse [lite]` | **2.132** | 1.833 | 0.585 | 2.676 | x1.29 |
| `selectolax [strip_tags]` | **2.550** | 2.312 | 1.117 | 3.804 | x1.54 |
| `oxide_parse [scraper]` | **10.611** | 7.701 | 4.274 | 13.550 | x6.41 |
| `bs4 + lxml [decompose]` | **124.180** | 114.159 | 12.486 | 161.573 | x75.01 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.397** | 0.287 | 0.129 | 0.683 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.616** | 1.164 | 0.351 | 2.471 | x4.07 |
| `scrapling [SQLite memory] (v1 match)` | **4.630** | 3.837 | 2.055 | 6.654 | x11.67 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.434** | 2.089 | 1.461 | 3.907 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **4.104** | 3.223 | 1.115 | 5.497 | x1.69 |
| `scrapling [SQLite memory] (v2 relocate)` | **652.069** | 566.982 | 219.235 | 949.868 | x267.87 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.169** | 0.942 | 0.288 | 1.729 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **5.904** | 5.728 | 0.361 | 6.272 | x5.05 |
| `scrapling [SQLite memory] (v1 match)` | **20.672** | 15.393 | 3.762 | 21.936 | x17.68 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **8.558** | 7.583 | 1.892 | 10.028 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **12.822** | 11.973 | 0.840 | 14.794 | x1.50 |
| `scrapling [SQLite memory] (v2 relocate)` | **3987.247** | 3930.237 | 619.144 | 4770.326 | x465.91 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **37.910** | 34.262 | 5.420 | 49.436 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **43.542** | 42.793 | 6.302 | 79.062 | x1.15 |
| `oxide_parse [xml] extract_many (Rayon)` | **47.599** | 39.762 | 16.363 | 56.560 | x1.26 |
| `oxide_parse [stream] sequential loop` | **117.181** | 97.674 | 86.781 | 194.556 | x3.09 |
| `oxide_parse [xml] sequential loop` | **119.844** | 87.511 | 47.494 | 143.303 | x3.16 |
| `oxide_parse [lite] sequential loop` | **125.585** | 102.458 | 54.024 | 180.571 | x3.31 |
| `oxide_parse [scraper] extract_many (Rayon)` | **280.270** | 244.013 | 55.693 | 527.262 | x7.39 |
| `selectolax [lexbor] sequential loop` | **436.736** | 373.719 | 207.583 | 831.029 | x11.52 |
| `oxide_parse [scraper] sequential loop` | **699.217** | 555.390 | 192.332 | 1053.334 | x18.44 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.017** | 2.682 | 0.779 | 3.515 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **3.461** | 3.300 | 2.560 | 7.302 | x1.15 |
| `xml: fast ASCII ('$19.99')` | **3.718** | 3.128 | 1.972 | 6.598 | x1.23 |
| `stream: localized ('1 299,50 ₽')` | **4.391** | 3.502 | 3.029 | 7.951 | x1.46 |
| `lite: localized ('1 299,50 ₽')` | **4.568** | 4.039 | 3.842 | 9.119 | x1.51 |
| `xml: localized ('1 299,50 ₽')` | **4.686** | 3.868 | 3.480 | 8.090 | x1.55 |
| `scraper: fast ASCII ('$19.99')` | **9.452** | 8.785 | 11.321 | 21.349 | x3.13 |
| `scraper: localized ('1 299,50 ₽')` | **11.764** | 10.152 | 7.099 | 23.498 | x3.90 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **4.118** | 3.938 | 1.551 | 5.910 | x1.00 |
| `xml: 2 selectors (run_native)` | **4.483** | 3.746 | 1.003 | 5.181 | x1.09 |
| `lite: 2 selectors (run_native)` | **4.965** | 4.837 | 0.318 | 7.064 | x1.21 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **6.763** | 6.565 | 1.158 | 9.395 | x1.64 |
| `xml: 11 selectors incl. 5 misses (run_native)` | **7.912** | 7.217 | 1.507 | 10.479 | x1.92 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **8.961** | 8.287 | 2.154 | 12.314 | x2.18 |
| `scraper: 2 selectors (run_native)` | **35.064** | 31.544 | 18.230 | 53.402 | x8.51 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **39.747** | 37.256 | 2.591 | 40.800 | x9.65 |

### HTML -> Markdown for RAG (300 blocks, 103.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [xml]` | **0.786** | 0.739 | 0.270 | 1.070 | x1.00 |
| `oxide_parse [lite]` | **0.930** | 0.850 | 0.311 | 1.240 | x1.18 |
| `oxide_parse [scraper]` | **5.040** | 3.694 | 2.802 | 12.093 | x6.41 |
| `html2text` | **76.312** | 73.059 | 45.770 | 145.892 | x97.10 |
| `markdownify` | **129.224** | 117.899 | 78.267 | 233.718 | x164.43 |

### Scrapling Bench: 5000 Nested Elements (408.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **4.361** | 3.301 | 1.212 | 5.468 | x1.00 |
| `oxide_parse [xml]` | **4.603** | 3.789 | 0.707 | 5.376 | x1.06 |
| `oxide_parse [lite]` | **5.410** | 4.345 | 2.474 | 8.825 | x1.24 |
| `frostwork` | **5.861** | 4.044 | 2.943 | 8.900 | x1.34 |
| `selectolax [lexbor]` | **8.865** | 7.694 | 3.061 | 13.385 | x2.03 |
| `oxide_parse [scraper]` | **25.038** | 20.247 | 12.740 | 38.907 | x5.74 |
| `lxml [direct]` | **44.158** | 33.711 | 21.924 | 67.058 | x10.13 |
| `scrapling` | **103.539** | 89.423 | 28.452 | 173.729 | x23.74 |
| `parsel` | **129.286** | 93.840 | 63.640 | 204.803 | x29.64 |
| `bs4 + lxml` | **417.739** | 374.597 | 157.169 | 558.059 | x95.78 |

### Scrapling Bench: find_similar vs adaptive list on 500 cards (52.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (adaptive list)` | **5.817** | 4.784 | 1.744 | 8.344 | x1.00 |
| `oxide_parse [scraper] (adaptive list)` | **7.882** | 7.099 | 1.134 | 9.219 | x1.36 |
| `scrapling [find_similar]` | **38.402** | 35.996 | 5.755 | 46.020 | x6.60 |

### Selectolax Bench: Strip script/style + body text + 800 links (83.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.986** | 0.943 | 0.133 | 3.045 | x1.00 |
| `oxide_parse [xml]` | **1.085** | 0.981 | 0.431 | 1.442 | x1.10 |
| `oxide_parse [lite]` | **1.292** | 1.112 | 0.396 | 3.339 | x1.31 |
| `selectolax [lexbor]` | **1.812** | 1.660 | 0.487 | 3.051 | x1.84 |
| `oxide_parse [scraper]` | **4.759** | 4.038 | 0.969 | 6.053 | x4.83 |
| `lxml [direct]` | **10.957** | 10.037 | 0.598 | 12.119 | x11.12 |
| `parsel` | **15.574** | 14.455 | 3.079 | 37.770 | x15.80 |
| `bs4 + lxml` | **89.473** | 86.222 | 9.188 | 96.229 | x90.79 |

### Frostwork Bench: Grouped Page (header + 2000 offers, 334.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **7.255** | 5.376 | 0.583 | 12.205 | x1.00 |
| `oxide_parse [xml]` | **9.041** | 8.140 | 1.500 | 10.769 | x1.25 |
| `oxide_parse [lite]` | **9.339** | 8.628 | 1.343 | 10.851 | x1.29 |
| `frostwork` | **12.409** | 12.135 | 0.645 | 14.678 | x1.71 |
| `oxide_parse [scraper]` | **28.972** | 25.001 | 5.484 | 34.772 | x3.99 |
| `selectolax [lexbor]` | **55.495** | 43.303 | 5.261 | 63.372 | x7.65 |
| `parsel` | **184.127** | 174.820 | 11.291 | 213.026 | x25.38 |
| `scrapling` | **283.155** | 274.350 | 14.189 | 335.035 | x39.03 |

### Frostwork Bench: first_only Early Exit (1591 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.079** | 0.070 | 0.008 | 0.094 | x1.00 |
| `frostwork [first_only]` | **0.249** | 0.229 | 0.018 | 0.336 | x3.15 |
| `oxide_parse [xml]` | **9.423** | 7.342 | 1.865 | 13.989 | x119.13 |
| `oxide_parse [lite]` | **10.742** | 9.048 | 5.183 | 15.525 | x135.80 |
| `selectolax [lexbor]` | **17.482** | 13.723 | 6.848 | 32.728 | x221.01 |
| `oxide_parse [scraper]` | **72.722** | 57.988 | 22.025 | 84.933 | x919.37 |
| `parsel` | **187.080** | 161.281 | 68.427 | 283.858 | x2365.11 |

### Parsel/Scrapy Quotes Bench (500 quotes with nested tags, 153.0 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **3.866** | 3.400 | 0.938 | 4.433 | x1.00 |
| `frostwork` | **5.342** | 4.562 | 0.790 | 6.059 | x1.38 |
| `oxide_parse [scraper]` | **11.059** | 10.329 | 2.842 | 18.842 | x2.86 |
| `selectolax [lexbor]` | **20.843** | 18.744 | 7.815 | 28.137 | x5.39 |
| `lxml [direct]` | **45.800** | 40.342 | 8.887 | 62.321 | x11.85 |
| `parsel` | **80.028** | 73.786 | 12.154 | 116.890 | x20.70 |
| `scrapling` | **112.150** | 103.350 | 17.172 | 149.468 | x29.01 |
| `bs4 + lxml` | **263.394** | 241.394 | 32.469 | 283.891 | x68.14 |

### Crawl4AI Nested E-commerce Catalog (30x4 products, 92.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.272** | 3.954 | 0.656 | 5.234 | x1.00 |
| `oxide_parse [scraper]` | **9.950** | 7.971 | 2.306 | 14.617 | x2.33 |
| `selectolax [lexbor]` | **18.772** | 17.089 | 1.125 | 21.094 | x4.39 |
| `parsel` | **73.422** | 67.609 | 4.584 | 87.025 | x17.18 |
| `crawl4ai [JsonXPath]` | **116.159** | 109.165 | 11.765 | 137.283 | x27.19 |
| `crawl4ai [JsonCss]` | **263.452** | 236.735 | 43.075 | 296.219 | x61.66 |


## Skipped Implementations

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

One warmup run, randomized order per round, GC run before timing, and all outputs verified. Cooldown pauses run between scenario groups. Sizes are in UTF-8 bytes.

OS background load, thermals, and CPU clocks were not pinned. `--quick` checks functionality on smaller catalogs.

## Crawl4AI prompt replacement

On [Books to Scrape](https://books.toscrape.com/), Crawl4AI 0.9.6 used `patch_crawl4ai_prompt(output="ir")` with Gemini 3.5 Flash-Lite. Providing a sample record to `target_json_example` skipped field inference. Upstream validation was enabled with `max_refinements=0`. The harness confirmed one schema-generation call and that the original prompt builder was restored on exit.

Generation and validation took **10.554 s** and the provider reported **5,417 total tokens**. The first response compiled directly with `Plan.from_ir()`. On all 20 books, `lite`, `scraper`, and `auto` matched an independent Beautiful Soup reference field for field and in order. `auto` selected `scraper`.

```json
{
  "pipe": [
    {"css": "article.product_pod"},
    {"map": {"object": {
      "title": {"pipe": [{"css": "h3 a"}, {"attr": "title"}]},
      "price": {"pipe": [{"css": ".price_color"}, "text", "strip"]},
      "availability": {"pipe": [{"css": ".availability"}, "text", "strip"]},
      "href": {"pipe": [{"css": "h3 a"}, {"attr": "href"}]}
    }}}
  ]
}
```

### Extraction with the generated schema

We reused that IR on 1,000 synthetic book cards (172,793 bytes) and checked all four fields on every iteration. Each series used 15 randomized rounds. The timed interval covered parsing and extraction from in-memory HTML with a precompiled plan or a preconfigured Crawl4AI strategy.

| Implementation | Median (ms) | IQR (ms) |
| --- | ---: | ---: |
| oxide_parse lite | 6.199 | 0.887 |
| oxide_parse scraper | 17.396 | 4.592 |
| oxide_parse stream with object wrapper | 5.645 | 1.529 |
| Crawl4AI JsonCss | 450.059 | 113.852 |

The Crawl4AI baseline used equivalent selectors. The stream variant wrapped the IR under an object field and returned that field so the output matched the original list. HTTP and Gemini were outside this measurement.

### Comparison with the previous full run

All 229 series across 15 groups and 26 result sections passed output validation. The median for 10,000 products on `stream` moved from 82.5 to 46.4 ms without noise and from 112.2 to 87.4 ms with noise. The native module SHA-256 matched the previous run, and shared competitor timings also fell. Separate sessions on this machine show timing variability.
