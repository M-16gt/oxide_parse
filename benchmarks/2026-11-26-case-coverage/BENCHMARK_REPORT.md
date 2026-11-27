# `oxide_parse` Full Benchmark Report — 2026-11-26

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
| Items | Noise | KiB | oxide_parse [scraper] | oxide_parse [lite] | oxide_parse [stream] | selectolax [lexbor] | frostwork | lxml [direct] | parsel | scrapling | bs4 + lxml |
|---:|:---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | False | 24.6 | 1.647 | 0.654 | **0.604** | 4.300 | 0.896 | 9.463 | 18.100 | 27.192 | 44.934 |
| 100 | True | 61.0 | 3.204 | 0.981 | **0.912** | 5.583 | 2.005 | 11.044 | 18.683 | 29.256 | 98.308 |
| 1000 | False | 249.4 | 17.626 | 6.572 | **5.062** | 36.148 | 9.197 | 71.721 | 163.950 | 253.577 | 388.535 |
| 1000 | True | 613.7 | 51.109 | 9.530 | **8.472** | 54.578 | 14.157 | 110.377 | 215.611 | 326.221 | 895.268 |
| 10000 | False | 2532.0 | 201.879 | 69.304 | **53.349** | 352.103 | 95.548 | 680.064 | 1563.472 | 2280.627 | 4011.505 |
| 10000 | True | 6174.6 | 505.840 | 104.486 | **80.601** | 545.223 | 161.557 | 1095.258 | 2189.056 | 2912.375 | 10170.056 |

### `extract` vs `run_native` Comparison

Median deltas estimate Python object construction overhead. `run_native` also counts output values and frees the Rust `Value`, so negative deltas can appear on noisy runs.

| Items | Noise | Engine | extract (ms) | run_native (ms) | Delta (ms) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.871 | 1.824 | 0.048 |
| 100 | False | `lite` | 0.588 | 0.672 | -0.085 |
| 100 | False | `stream` | 0.579 | 0.511 | 0.068 |
| 100 | True | `scraper` | 3.945 | 3.768 | 0.177 |
| 100 | True | `lite` | 1.064 | 0.839 | 0.225 |
| 100 | True | `stream` | 0.675 | 0.654 | 0.021 |
| 1000 | False | `scraper` | 16.152 | 16.715 | -0.563 |
| 1000 | False | `lite` | 6.726 | 6.102 | 0.624 |
| 1000 | False | `stream` | 5.044 | 4.494 | 0.550 |
| 1000 | True | `scraper` | 52.465 | 50.127 | 2.338 |
| 1000 | True | `lite` | 9.527 | 7.141 | 2.386 |
| 1000 | True | `stream` | 8.997 | 7.063 | 1.934 |
| 10000 | False | `scraper` | 209.542 | 191.461 | 18.081 |
| 10000 | False | `lite` | 61.270 | 53.586 | 7.685 |
| 10000 | False | `stream` | 53.615 | 48.964 | 4.651 |
| 10000 | True | `scraper` | 471.329 | 407.365 | 63.964 |
| 10000 | True | `lite` | 100.786 | 82.426 | 18.361 |
| 10000 | True | `stream` | 79.726 | 78.439 | 1.288 |

## 2. Scenario Results
### Early-Exit Head + 1st Offer on 1591 KiB page
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.098** | 0.076 | 0.053 | 0.267 | x1.00 |
| `oxide_parse [lite]` | **7.790** | 6.175 | 1.136 | 9.269 | x79.49 |
| `selectolax [lexbor]` | **15.800** | 11.680 | 1.726 | 29.632 | x161.22 |
| `oxide_parse [scraper]` | **59.496** | 43.836 | 22.695 | 96.589 | x607.10 |
| `lxml [direct]` | **162.569** | 128.733 | 15.074 | 172.817 | x1658.87 |
| `parsel` | **185.380** | 141.921 | 47.661 | 222.153 | x1891.63 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.376** | 2.177 | 0.940 | 3.373 | x1.00 |
| `oxide_parse [lite]` | **2.406** | 2.293 | 0.304 | 2.806 | x1.01 |
| `selectolax [lexbor]` | **4.621** | 3.145 | 1.854 | 8.899 | x1.95 |
| `oxide_parse [scraper]` | **10.694** | 9.424 | 1.850 | 11.588 | x4.50 |
| `parsel` | **48.521** | 29.360 | 21.610 | 53.376 | x20.42 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.803** | 4.783 | 1.757 | 6.661 | x1.00 |
| `oxide_parse [scraper]` | **11.651** | 9.037 | 3.931 | 18.941 | x2.43 |
| `lxml [xpath]` | **12.018** | 10.599 | 5.935 | 23.569 | x2.50 |
| `parsel [xpath]` | **18.646** | 13.595 | 17.795 | 34.477 | x3.88 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.207** | 2.070 | 0.923 | 4.629 | x1.00 |
| `oxide_parse [lite]` | **2.599** | 2.103 | 0.620 | 3.528 | x1.18 |
| `selectolax + json.loads` | **3.122** | 2.031 | 1.138 | 3.700 | x1.41 |
| `oxide_parse [scraper]` | **3.583** | 2.717 | 1.010 | 7.677 | x1.62 |
| `lxml + json.loads` | **5.224** | 4.637 | 2.968 | 7.930 | x2.37 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.340** | 1.789 | 1.406 | 5.980 | x1.00 |
| `oxide_parse [lite]` | **3.005** | 1.872 | 1.147 | 3.466 | x1.28 |
| `selectolax [strip_tags]` | **3.101** | 2.300 | 1.263 | 5.272 | x1.33 |
| `oxide_parse [scraper]` | **11.998** | 10.817 | 2.182 | 14.684 | x5.13 |
| `bs4 + lxml [decompose]` | **183.137** | 174.416 | 29.409 | 219.242 | x78.27 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.273** | 0.264 | 0.234 | 1.033 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.413** | 1.157 | 0.859 | 5.104 | x5.18 |
| `scrapling [SQLite memory] (v1 match)` | **5.692** | 4.091 | 2.658 | 21.484 | x20.89 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.796** | 2.103 | 0.949 | 3.342 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **3.676** | 2.950 | 3.068 | 7.077 | x1.31 |
| `scrapling [SQLite memory] (v2 relocate)` | **701.665** | 609.764 | 109.413 | 819.092 | x250.99 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.378** | 1.094 | 0.383 | 2.042 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **8.307** | 6.178 | 2.651 | 12.249 | x6.03 |
| `scrapling [SQLite memory] (v1 match)` | **23.694** | 21.602 | 6.351 | 30.856 | x17.19 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **9.487** | 8.119 | 2.089 | 11.106 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **14.819** | 12.914 | 2.333 | 17.549 | x1.56 |
| `scrapling [SQLite memory] (v2 relocate)` | **4908.482** | 4822.476 | 148.929 | 5055.578 | x517.38 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **40.129** | 33.107 | 6.453 | 46.966 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **51.564** | 40.145 | 14.578 | 59.537 | x1.28 |
| `oxide_parse [stream] sequential loop` | **102.242** | 90.745 | 28.495 | 151.644 | x2.55 |
| `oxide_parse [lite] sequential loop` | **141.815** | 98.540 | 15.851 | 143.727 | x3.53 |
| `oxide_parse [scraper] extract_many (Rayon)` | **275.912** | 246.368 | 64.517 | 350.784 | x6.88 |
| `selectolax [lexbor] sequential loop` | **494.048** | 367.281 | 87.395 | 536.844 | x12.31 |
| `oxide_parse [scraper] sequential loop` | **749.406** | 621.900 | 100.984 | 762.934 | x18.68 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.091** | 2.595 | 1.053 | 3.879 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **3.973** | 3.226 | 0.897 | 5.143 | x1.29 |
| `lite: localized ('1 299,50 ₽')` | **5.136** | 4.810 | 1.197 | 7.047 | x1.66 |
| `stream: localized ('1 299,50 ₽')` | **5.163** | 3.491 | 0.821 | 6.430 | x1.67 |
| `scraper: fast ASCII ('$19.99')` | **11.847** | 10.185 | 4.253 | 17.341 | x3.83 |
| `scraper: localized ('1 299,50 ₽')` | **12.838** | 10.750 | 4.380 | 17.629 | x4.15 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **4.662** | 3.787 | 1.504 | 5.500 | x1.00 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **8.095** | 6.844 | 1.943 | 9.364 | x1.74 |
| `lite: 2 selectors (run_native)` | **8.236** | 5.130 | 1.638 | 47.194 | x1.77 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **10.858** | 7.985 | 3.079 | 14.492 | x2.33 |
| `scraper: 2 selectors (run_native)` | **46.590** | 35.310 | 21.584 | 63.084 | x9.99 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **53.184** | 40.012 | 24.518 | 69.715 | x11.41 |

### HTML -> Markdown for RAG (300 blocks, 103.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **1.089** | 0.883 | 0.341 | 1.280 | x1.00 |
| `oxide_parse [scraper]` | **4.862** | 3.833 | 2.059 | 6.621 | x4.46 |
| `html2text` | **91.959** | 74.965 | 17.778 | 114.158 | x84.44 |
| `markdownify` | **138.101** | 116.533 | 37.922 | 174.517 | x126.81 |

### Scrapling Bench: 5000 Nested Elements (408.5 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **4.505** | 3.472 | 3.654 | 10.222 | x1.00 |
| `oxide_parse [lite]` | **4.866** | 3.672 | 2.246 | 7.862 | x1.08 |
| `frostwork` | **5.292** | 3.899 | 2.690 | 7.390 | x1.17 |
| `selectolax [lexbor]` | **8.877** | 6.248 | 8.419 | 16.712 | x1.97 |
| `oxide_parse [scraper]` | **22.912** | 17.881 | 14.675 | 36.523 | x5.09 |
| `lxml [direct]` | **42.141** | 31.953 | 21.180 | 69.765 | x9.35 |
| `scrapling` | **85.692** | 80.182 | 28.541 | 121.897 | x19.02 |
| `parsel` | **109.749** | 82.613 | 11.346 | 117.187 | x24.36 |
| `bs4 + lxml` | **438.677** | 355.108 | 31.715 | 526.355 | x97.36 |

### Scrapling Bench: find_similar vs adaptive list on 500 cards (52.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (adaptive list)` | **5.955** | 5.027 | 0.146 | 6.590 | x1.00 |
| `oxide_parse [scraper] (adaptive list)` | **8.401** | 6.812 | 2.406 | 9.618 | x1.41 |
| `scrapling [find_similar]` | **44.798** | 32.811 | 17.992 | 72.894 | x7.52 |

### Selectolax Bench: Strip script/style + body text + 800 links (83.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.776** | 0.725 | 0.204 | 0.984 | x1.00 |
| `oxide_parse [lite]` | **1.062** | 0.935 | 0.465 | 1.557 | x1.37 |
| `selectolax [lexbor]` | **1.766** | 1.287 | 0.439 | 1.873 | x2.28 |
| `oxide_parse [scraper]` | **4.051** | 3.257 | 1.758 | 5.389 | x5.22 |
| `lxml [direct]` | **9.580** | 7.890 | 5.962 | 16.597 | x12.35 |
| `parsel` | **15.074** | 10.456 | 4.290 | 17.759 | x19.43 |
| `bs4 + lxml` | **91.773** | 68.300 | 49.386 | 124.040 | x118.28 |

### Frostwork Bench: Grouped Page (header + 2000 offers, 334.2 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **6.233** | 5.432 | 3.024 | 12.876 | x1.00 |
| `oxide_parse [lite]` | **7.305** | 6.993 | 1.915 | 10.806 | x1.17 |
| `frostwork` | **11.178** | 9.631 | 2.413 | 12.986 | x1.79 |
| `oxide_parse [scraper]` | **29.454** | 25.171 | 5.483 | 39.060 | x4.73 |
| `selectolax [lexbor]` | **44.173** | 40.192 | 11.915 | 55.997 | x7.09 |
| `parsel` | **163.756** | 142.004 | 37.693 | 249.245 | x26.27 |
| `scrapling` | **295.199** | 233.601 | 104.092 | 350.872 | x47.36 |

### Frostwork Bench: first_only Early Exit (1591 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **0.071** | 0.069 | 0.013 | 0.725 | x1.00 |
| `frostwork [first_only]` | **0.228** | 0.207 | 0.102 | 0.532 | x3.20 |
| `oxide_parse [lite]` | **7.003** | 5.646 | 1.673 | 10.853 | x98.50 |
| `selectolax [lexbor]` | **17.204** | 13.434 | 5.924 | 22.005 | x241.97 |
| `oxide_parse [scraper]` | **59.230** | 43.378 | 27.999 | 78.500 | x833.06 |
| `parsel` | **183.184** | 153.339 | 40.884 | 214.494 | x2576.43 |

### Parsel/Scrapy Quotes Bench (500 quotes with nested tags, 153.0 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **3.811** | 2.858 | 1.155 | 5.502 | x1.00 |
| `frostwork` | **5.122** | 4.104 | 0.767 | 6.541 | x1.34 |
| `oxide_parse [scraper]` | **10.876** | 8.183 | 7.433 | 23.203 | x2.85 |
| `selectolax [lexbor]` | **15.100** | 12.028 | 5.788 | 18.257 | x3.96 |
| `lxml [direct]` | **40.829** | 34.011 | 8.225 | 46.110 | x10.71 |
| `parsel` | **67.687** | 58.619 | 32.770 | 99.196 | x17.76 |
| `scrapling` | **103.328** | 91.163 | 33.009 | 138.688 | x27.11 |
| `bs4 + lxml` | **216.078** | 200.774 | 31.591 | 292.776 | x56.70 |

### Crawl4AI Nested E-commerce Catalog (30x4 products, 92.9 KiB)
| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.206** | 4.048 | 1.294 | 9.015 | x1.00 |
| `oxide_parse [scraper]` | **9.247** | 6.726 | 3.874 | 11.080 | x2.20 |
| `selectolax [lexbor]` | **16.198** | 13.574 | 7.238 | 30.686 | x3.85 |
| `parsel` | **72.931** | 55.340 | 18.435 | 75.284 | x17.34 |
| `crawl4ai [JsonXPath]` | **107.314** | 94.884 | 47.072 | 156.412 | x25.52 |
| `crawl4ai [JsonCss]` | **281.551** | 242.068 | 63.883 | 331.570 | x66.94 |


## Skipped Implementations

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

One warmup run, randomized order per round, GC run before timing, and all outputs verified. Cooldown pauses run between scenario groups. Sizes are in UTF-8 bytes.

OS background load, thermals, and CPU clocks were not pinned. `--quick` checks functionality on smaller catalogs.

## Дополнительные замеры новых API

Медиана и IQR в мс. 21 замер на вариант, случайный порядок вариантов внутри раунда и проверка одинакового результата после каждого замера. Планы скомпилированы заранее. HTTP-ответы в тесте пагинации подаются из памяти.

| Вход и операция | Вариант | Медиана | IQR |
|---|---|---:|---:|
| 1000 HTML-элементов внутри JSON | parse_html | 1.225 | 0.280 |
| 1000 HTML-элементов внутри JSON | Два плана JSON и HTML (auto) | 1.237 | 0.455 |
| HTML → JSON, 30 страниц по 100 элементов, Link | follow(format="json") + paginate | 10.229 | 3.480 |
| HTML → JSON, 30 страниц по 100 элементов, Link | Отдельные HTML- и JSON-планы | 10.928 | 3.681 |
| 1000 элементов, HTML Windows-1251 | Автоопределение по meta charset | 0.787 | 0.117 |
| 1000 элементов, HTML Windows-1251 | Явный encoding="cp1251" | 0.743 | 0.129 |
| 1000 элементов, HTML Windows-1251 | Декодирование в Python перед extract | 0.735 | 0.099 |

Новые API дают тот же результат, что и явная обвязка на этих входах. Разницы между медианами меньше межквартильного разброса.

## Проверка корректности

- 24 новых теста по кейсам прошли.
- Общий Python-прогон с установленным Crawl4AI 0.9.6: 1366 passed, 110 skipped, 13 xfailed. Живой Gemini-тест исключён из этого прогона.
- Rust: 116 тестов, включая doctest. Отдельные сборки engine-scraper, engine-lite и fast прошли.
- 15 групп, 26 вариантов нагрузки и 196 серий основного набора завершились с проверкой результатов.

Основной набор воспроизводится на релизном колесе со всеми движками командой:

```sh
python -u benches/suite_v7/run_all_benchmarks.py --full --cooldown 2 --output benchmarks/local_run
```
