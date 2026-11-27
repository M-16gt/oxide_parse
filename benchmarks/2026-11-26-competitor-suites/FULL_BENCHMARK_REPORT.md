# Полный отчет бенчмарков `oxide_parse` — 2026-11-26

## Окружение и версии библиотек
- **ОС / Платформа:** `Windows-11-10.0.26200-SP0`
- **Процессор / Ядра:** `Intel64 Family 6 Model 190 Stepping 0, GenuineIntel` (4 лог. ядер)
- **Python:** `3.13.7 (tags/v3.13.7:bcee1c3, Aug 14 2025, 14:15:11) [MSC v.1944 64 bit (AMD64)]`
- **Параметры прогона:** `repeat=7`, `cooldown=2.0s`
- **Обнаруженные библиотеки:**
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

## 1. Сводная таблица: Каталог товаров (Медианы, мс)
| Товаров | Noise | KiB | oxide_parse [scraper] | oxide_parse [lite] | oxide_parse [stream] | selectolax [lexbor] | frostwork | lxml [direct] | parsel | scrapling | bs4 + lxml |
|---:|:---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | False | 24.6 | 1.635 | 0.677 | **0.597** | 2.929 | 0.954 | 6.607 | 17.225 | 24.000 | 39.430 |
| 100 | True | 61.0 | 3.898 | 1.024 | **0.927** | 4.187 | 1.733 | 12.452 | 23.861 | 29.142 | 100.160 |
| 1000 | False | 249.4 | 19.296 | 7.136 | **5.979** | 37.530 | 9.701 | 87.121 | 189.087 | 267.704 | 466.574 |
| 1000 | True | 613.7 | 47.761 | 10.482 | **8.777** | 46.544 | 14.042 | 116.854 | 227.629 | 301.179 | 952.026 |
| 10000 | False | 2532.0 | 217.841 | 78.685 | **66.501** | 425.576 | 104.556 | 752.363 | 1782.278 | 2580.923 | 4415.675 |
| 10000 | True | 6174.6 | 484.255 | 107.496 | **90.441** | 541.147 | 177.154 | 1239.101 | 2327.953 | 3043.162 | 10320.385 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.804 | 1.973 | -0.169 |
| 100 | False | `lite` | 0.612 | 0.548 | 0.064 |
| 100 | False | `stream` | 0.640 | 0.579 | 0.061 |
| 100 | True | `scraper` | 8.790 | 5.577 | 3.212 |
| 100 | True | `lite` | 1.325 | 0.897 | 0.428 |
| 100 | True | `stream` | 0.960 | 0.959 | 0.001 |
| 1000 | False | `scraper` | 22.212 | 20.739 | 1.473 |
| 1000 | False | `lite` | 6.399 | 6.141 | 0.258 |
| 1000 | False | `stream` | 4.765 | 4.678 | 0.087 |
| 1000 | True | `scraper` | 46.972 | 47.470 | -0.498 |
| 1000 | True | `lite` | 9.458 | 9.539 | -0.081 |
| 1000 | True | `stream` | 8.950 | 8.439 | 0.511 |
| 10000 | False | `scraper` | 220.049 | 196.060 | 23.989 |
| 10000 | False | `lite` | 71.979 | 72.797 | -0.818 |
| 10000 | False | `stream` | 65.935 | 53.437 | 12.498 |
| 10000 | True | `scraper` | 522.251 | 517.369 | 4.881 |
| 10000 | True | `lite` | 160.593 | 149.980 | 10.613 |
| 10000 | True | `stream` | 137.848 | 108.279 | 29.569 |

## 2. Результаты по всем специальным сценариям
### Early-Exit Head + 1st Offer on 1591 KiB page
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.836** | 2.573 | 0.576 | 3.870 | x1.00 |
| `oxide_parse [lite]` | **13.209** | 10.691 | 5.878 | 19.426 | x4.66 |
| `selectolax [lexbor]` | **19.930** | 16.917 | 6.803 | 26.323 | x7.03 |
| `oxide_parse [scraper]` | **62.715** | 56.171 | 9.108 | 76.746 | x22.11 |
| `lxml [direct]` | **176.224** | 142.055 | 27.726 | 211.727 | x62.13 |
| `parsel` | **233.713** | 185.668 | 33.862 | 248.173 | x82.40 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.333** | 2.175 | 1.677 | 4.796 | x1.00 |
| `oxide_parse [stream]` | **3.012** | 2.412 | 2.662 | 7.812 | x1.29 |
| `selectolax [lexbor]` | **3.353** | 3.193 | 1.234 | 4.667 | x1.44 |
| `oxide_parse [scraper]` | **10.412** | 8.099 | 4.667 | 14.368 | x4.46 |
| `parsel` | **39.315** | 30.043 | 23.865 | 58.952 | x16.85 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **6.629** | 5.749 | 1.135 | 8.957 | x1.00 |
| `oxide_parse [scraper]` | **12.504** | 11.557 | 3.525 | 20.370 | x1.89 |
| `lxml [xpath]` | **13.777** | 11.733 | 6.146 | 20.684 | x2.08 |
| `parsel [xpath]` | **19.546** | 16.899 | 2.936 | 20.621 | x2.95 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.214** | 1.967 | 1.624 | 3.699 | x1.00 |
| `oxide_parse [lite]` | **4.152** | 3.726 | 1.115 | 13.413 | x1.87 |
| `oxide_parse [scraper]` | **5.557** | 4.791 | 1.018 | 8.217 | x2.51 |
| `lxml + json.loads` | **6.117** | 4.474 | 3.230 | 8.893 | x2.76 |
| `oxide_parse [stream]` | **6.449** | 5.105 | 3.098 | 9.968 | x2.91 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.535** | 2.292 | 0.369 | 2.744 | x1.00 |
| `oxide_parse [lite]` | **2.915** | 2.461 | 0.450 | 7.211 | x1.15 |
| `selectolax [strip_tags]` | **3.365** | 2.751 | 2.479 | 6.640 | x1.33 |
| `oxide_parse [scraper]` | **11.255** | 9.724 | 7.131 | 17.352 | x4.44 |
| `bs4 + lxml [decompose]` | **150.621** | 144.430 | 16.231 | 189.282 | x59.41 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.373** | 0.353 | 0.070 | 0.557 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.424** | 1.393 | 0.259 | 1.735 | x3.82 |
| `scrapling [SQLite memory] (v1 match)` | **5.426** | 4.697 | 1.341 | 8.240 | x14.56 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.592** | 2.557 | 2.070 | 5.981 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **4.165** | 3.636 | 2.879 | 6.713 | x1.61 |
| `scrapling [SQLite memory] (v2 relocate)` | **714.067** | 674.502 | 80.165 | 774.206 | x275.48 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.380** | 1.145 | 0.940 | 2.365 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **5.888** | 5.530 | 1.526 | 7.486 | x4.27 |
| `scrapling [SQLite memory] (v1 match)` | **19.441** | 17.932 | 3.758 | 24.278 | x14.09 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **11.110** | 8.883 | 1.193 | 12.049 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **16.213** | 15.326 | 4.329 | 20.237 | x1.46 |
| `scrapling [SQLite memory] (v2 relocate)` | **5416.482** | 5267.978 | 607.975 | 6102.416 | x487.51 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **47.608** | 38.156 | 13.197 | 61.973 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **48.903** | 45.333 | 11.565 | 70.900 | x1.03 |
| `oxide_parse [stream] sequential loop` | **138.242** | 109.331 | 40.501 | 230.725 | x2.90 |
| `oxide_parse [lite] sequential loop` | **144.087** | 129.501 | 13.422 | 154.101 | x3.03 |
| `oxide_parse [scraper] extract_many (Rayon)` | **291.361** | 257.106 | 41.772 | 310.430 | x6.12 |
| `selectolax [lexbor] sequential loop` | **541.363** | 491.615 | 61.376 | 601.154 | x11.37 |
| `oxide_parse [scraper] sequential loop` | **756.143** | 711.526 | 73.525 | 807.762 | x15.88 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.533** | 3.336 | 0.654 | 4.390 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **4.541** | 4.111 | 2.126 | 6.469 | x1.29 |
| `stream: localized ('1 299,50 ₽')` | **4.937** | 4.574 | 0.308 | 6.777 | x1.40 |
| `lite: localized ('1 299,50 ₽')` | **5.899** | 4.402 | 1.448 | 9.303 | x1.67 |
| `scraper: localized ('1 299,50 ₽')` | **12.867** | 11.338 | 5.283 | 18.564 | x3.64 |
| `scraper: fast ASCII ('$19.99')` | **13.627** | 11.109 | 4.724 | 20.498 | x3.86 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **6.122** | 5.409 | 1.809 | 8.590 | x1.00 |
| `lite: 2 selectors (run_native)` | **7.162** | 6.186 | 2.691 | 22.102 | x1.17 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **10.433** | 9.905 | 2.562 | 19.567 | x1.70 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **12.637** | 11.754 | 2.805 | 30.427 | x2.06 |
| `scraper: 2 selectors (run_native)` | **47.384** | 37.824 | 9.090 | 58.310 | x7.74 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **60.719** | 45.474 | 17.774 | 78.305 | x9.92 |

### HTML -> Markdown for RAG (300 blocks, 103.5 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **1.134** | 1.116 | 0.485 | 1.762 | x1.00 |
| `oxide_parse [scraper]` | **4.994** | 4.550 | 0.809 | 7.432 | x4.40 |
| `html2text` | **104.982** | 90.250 | 16.083 | 115.255 | x92.60 |
| `markdownify` | **154.320** | 140.713 | 21.035 | 191.108 | x136.12 |

### Scrapling Bench: 5000 Nested Elements (408.5 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **5.269** | 4.197 | 1.574 | 7.999 | x1.00 |
| `oxide_parse [lite]` | **5.623** | 4.830 | 1.038 | 10.301 | x1.07 |
| `frostwork` | **5.748** | 4.131 | 1.155 | 6.377 | x1.09 |
| `selectolax [lexbor]` | **8.584** | 6.655 | 1.747 | 12.811 | x1.63 |
| `oxide_parse [scraper]` | **24.863** | 21.134 | 4.545 | 34.098 | x4.72 |
| `lxml [direct]` | **40.533** | 32.851 | 20.460 | 80.522 | x7.69 |
| `scrapling` | **121.820** | 98.130 | 45.838 | 153.827 | x23.12 |
| `parsel` | **127.310** | 101.905 | 29.136 | 176.679 | x24.16 |
| `bs4 + lxml` | **420.992** | 392.443 | 76.403 | 510.540 | x79.89 |

### Scrapling Bench: find_similar vs adaptive list on 500 cards (52.2 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (adaptive list)` | **6.266** | 5.898 | 3.263 | 12.945 | x1.00 |
| `oxide_parse [scraper] (adaptive list)` | **9.352** | 8.271 | 2.073 | 13.585 | x1.49 |
| `scrapling [find_similar]` | **41.869** | 37.907 | 10.809 | 50.755 | x6.68 |

### Selectolax Bench: Strip script/style + body text + 800 links (83.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **1.092** | 0.992 | 0.585 | 1.652 | x1.00 |
| `oxide_parse [lite]` | **1.139** | 1.127 | 0.157 | 1.806 | x1.04 |
| `selectolax [lexbor]` | **1.789** | 1.282 | 0.562 | 2.475 | x1.64 |
| `oxide_parse [scraper]` | **5.173** | 3.813 | 0.959 | 6.070 | x4.74 |
| `lxml [direct]` | **11.106** | 8.920 | 2.007 | 18.451 | x10.17 |
| `parsel` | **13.906** | 13.334 | 2.148 | 23.865 | x12.73 |
| `bs4 + lxml` | **86.647** | 85.463 | 28.661 | 153.749 | x79.35 |

### Frostwork Bench: Grouped Page (header + 2000 offers, 334.2 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **7.570** | 6.737 | 3.858 | 11.179 | x1.00 |
| `oxide_parse [lite]` | **10.257** | 8.854 | 6.030 | 16.683 | x1.35 |
| `frostwork` | **13.643** | 13.280 | 1.785 | 15.134 | x1.80 |
| `oxide_parse [scraper]` | **30.610** | 24.851 | 7.041 | 46.449 | x4.04 |
| `selectolax [lexbor]` | **55.416** | 51.840 | 15.015 | 78.983 | x7.32 |
| `parsel` | **184.227** | 174.264 | 27.204 | 220.689 | x24.34 |
| `scrapling` | **279.011** | 258.339 | 47.730 | 334.659 | x36.86 |

### Frostwork Bench: first_only Early Exit (1591 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `frostwork [first_only]` | **0.227** | 0.213 | 0.112 | 0.955 | x1.00 |
| `oxide_parse [stream]` | **1.153** | 0.984 | 1.114 | 2.110 | x5.07 |
| `oxide_parse [lite]` | **10.160** | 8.500 | 1.483 | 10.893 | x44.68 |
| `selectolax [lexbor]` | **15.614** | 14.375 | 2.533 | 19.209 | x68.66 |
| `oxide_parse [scraper]` | **67.685** | 51.929 | 25.803 | 97.447 | x297.65 |
| `parsel` | **189.173** | 169.948 | 39.382 | 253.107 | x831.90 |

### Parsel/Scrapy Quotes Bench (500 quotes with nested tags, 153.0 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **3.662** | 3.305 | 2.155 | 6.814 | x1.00 |
| `frostwork` | **4.519** | 3.551 | 1.913 | 5.586 | x1.23 |
| `oxide_parse [scraper]` | **11.315** | 9.914 | 3.425 | 14.736 | x3.09 |
| `selectolax [lexbor]` | **18.059** | 16.792 | 9.826 | 27.508 | x4.93 |
| `lxml [direct]` | **43.194** | 40.781 | 6.212 | 50.022 | x11.79 |
| `parsel` | **72.853** | 70.193 | 21.003 | 92.847 | x19.89 |
| `scrapling` | **117.724** | 102.865 | 18.630 | 130.101 | x32.15 |
| `bs4 + lxml` | **261.533** | 237.013 | 32.291 | 281.255 | x71.42 |

### Crawl4AI Nested E-commerce Catalog (30x4 products, 92.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **4.696** | 3.890 | 2.133 | 7.184 | x1.00 |
| `oxide_parse [scraper]` | **11.016** | 9.097 | 5.285 | 17.143 | x2.35 |
| `selectolax [lexbor]` | **23.070** | 18.241 | 5.494 | 39.188 | x4.91 |
| `parsel` | **76.231** | 67.471 | 18.627 | 116.552 | x16.23 |
| `crawl4ai [JsonXPath]` | **134.230** | 113.133 | 25.871 | 147.199 | x28.59 |
| `crawl4ai [JsonCss]` | **294.344** | 255.297 | 66.782 | 350.973 | x62.68 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
