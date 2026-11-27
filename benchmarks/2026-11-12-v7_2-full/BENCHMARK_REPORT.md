# Полный отчет бенчмарков `oxide_parse` — 2026-11-12

## Окружение и версии библиотек
- **ОС / Платформа:** `Windows-11-10.0.26200-SP0`
- **Процессор / Ядра:** `Intel64 Family 6 Model 190 Stepping 0, GenuineIntel` (4 лог. ядер)
- **Python:** `3.13.7 (tags/v3.13.7:bcee1c3, Aug 14 2025, 14:15:11) [MSC v.1944 64 bit (AMD64)]`
- **Параметры прогона:** `repeat=7`, `cooldown=1.0s`
- **Обнаруженные библиотеки:**
  - `bs4`: **4.15.0**
  - `frostwork`: **0.2.2**
  - `lxml`: **6.1.3**
  - `oxide_parse`: **0.1.0**
  - `parsel`: **1.12.1**
  - `scrapling`: **0.5.1**
  - `selectolax`: **1.0.2**

## 1. Сводная таблица: Каталог товаров (Медианы, мс)
| Товаров | Noise | KiB | oxide_parse [scraper] | oxide_parse [lite] | oxide_parse [stream] | selectolax [lexbor] | frostwork | lxml [direct] | parsel | scrapling | bs4 + lxml |
|---:|:---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | False | 24.6 | 2.431 | 0.748 | **0.618** | 4.852 | 1.176 | 8.536 | 25.984 | 36.338 | 60.330 |
| 100 | True | 61.0 | 4.264 | 0.973 | **0.908** | 4.283 | 1.517 | 10.298 | 21.034 | 25.859 | 85.934 |
| 1000 | False | 249.4 | 17.978 | 7.004 | **5.119** | 43.404 | 8.273 | 76.282 | 181.098 | 212.121 | 431.378 |
| 1000 | True | 613.7 | 41.088 | 9.383 | **7.545** | 37.327 | 13.010 | 94.931 | 175.635 | 254.866 | 808.983 |
| 10000 | False | 2532.0 | 154.940 | 55.464 | **41.821** | 302.207 | 77.603 | 575.635 | 1348.284 | 1944.392 | 3540.476 |
| 10000 | True | 6174.6 | 382.378 | 103.131 | **71.863** | 445.075 | 127.522 | 958.477 | 1899.556 | 2429.448 | 8659.598 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 2.050 | 1.795 | 0.255 |
| 100 | False | `lite` | 0.725 | 0.572 | 0.153 |
| 100 | False | `stream` | 0.601 | 0.498 | 0.103 |
| 100 | True | `scraper` | 3.614 | 3.382 | 0.232 |
| 100 | True | `lite` | 0.786 | 0.747 | 0.039 |
| 100 | True | `stream` | 0.728 | 0.645 | 0.083 |
| 1000 | False | `scraper` | 16.822 | 15.054 | 1.768 |
| 1000 | False | `lite` | 5.265 | 4.322 | 0.942 |
| 1000 | False | `stream` | 3.594 | 3.356 | 0.238 |
| 1000 | True | `scraper` | 36.512 | 37.739 | -1.227 |
| 1000 | True | `lite` | 8.947 | 8.355 | 0.593 |
| 1000 | True | `stream` | 6.766 | 5.983 | 0.783 |
| 10000 | False | `scraper` | 157.549 | 148.135 | 9.414 |
| 10000 | False | `lite` | 52.114 | 46.115 | 6.000 |
| 10000 | False | `stream` | 41.399 | 36.656 | 4.743 |
| 10000 | True | `scraper` | 380.804 | 360.253 | 20.551 |
| 10000 | True | `lite` | 87.446 | 82.727 | 4.719 |
| 10000 | True | `stream` | 69.536 | 63.899 | 5.636 |

## 2. Результаты по всем специальным сценариям
### Early-Exit Head + 1st Offer on 1591 KiB page
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.973** | 2.357 | 0.910 | 4.019 | x1.00 |
| `oxide_parse [lite]` | **12.806** | 7.896 | 7.883 | 17.124 | x4.31 |
| `selectolax [lexbor]` | **21.372** | 14.597 | 9.638 | 96.744 | x7.19 |
| `oxide_parse [scraper]` | **87.792** | 47.745 | 39.735 | 103.498 | x29.53 |
| `lxml [direct]` | **211.329** | 125.969 | 109.531 | 259.116 | x71.08 |
| `parsel` | **237.077** | 145.898 | 141.802 | 306.208 | x79.74 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.154** | 2.109 | 0.313 | 2.908 | x1.00 |
| `oxide_parse [stream]` | **2.357** | 1.842 | 0.257 | 2.530 | x1.09 |
| `selectolax [lexbor]` | **4.164** | 3.213 | 1.159 | 6.242 | x1.93 |
| `oxide_parse [scraper]` | **9.330** | 7.668 | 2.492 | 13.290 | x4.33 |
| `parsel` | **35.722** | 27.846 | 1.982 | 41.149 | x16.59 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **6.672** | 6.447 | 3.252 | 10.116 | x1.00 |
| `oxide_parse [scraper]` | **14.760** | 12.687 | 8.505 | 23.475 | x2.21 |
| `lxml [xpath]` | **17.170** | 14.193 | 3.154 | 22.096 | x2.57 |
| `parsel [xpath]` | **20.477** | 18.682 | 12.325 | 31.535 | x3.07 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.124** | 1.988 | 0.245 | 3.001 | x1.00 |
| `lxml + json.loads` | **5.922** | 4.082 | 1.615 | 8.736 | x2.79 |
| `oxide_parse [stream]` | **9.363** | 8.355 | 1.311 | 10.024 | x4.41 |
| `oxide_parse [lite]` | **10.768** | 9.410 | 1.211 | 11.175 | x5.07 |
| `oxide_parse [scraper]` | **11.455** | 10.717 | 1.193 | 12.281 | x5.39 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.151** | 1.904 | 0.752 | 2.799 | x1.00 |
| `selectolax [strip_tags]` | **4.245** | 3.163 | 2.044 | 5.869 | x1.97 |
| `oxide_parse [scraper]` | **12.155** | 11.345 | 1.065 | 13.673 | x5.65 |
| `bs4 + lxml [decompose]` | **169.524** | 160.361 | 10.171 | 180.384 | x78.81 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.389** | 0.316 | 0.089 | 0.498 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.692** | 1.235 | 0.427 | 2.160 | x4.36 |
| `scrapling [SQLite memory] (v1 match)` | **5.763** | 4.673 | 1.077 | 6.002 | x14.83 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.172** | 2.042 | 0.483 | 2.996 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **3.227** | 3.060 | 0.973 | 4.670 | x1.49 |
| `scrapling [SQLite memory] (v2 relocate)` | **571.060** | 546.218 | 45.763 | 1016.192 | x262.87 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.944** | 0.862 | 0.379 | 1.445 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **6.025** | 4.867 | 1.232 | 7.242 | x6.39 |
| `scrapling [SQLite memory] (v1 match)` | **15.408** | 14.607 | 2.369 | 18.580 | x16.33 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **8.902** | 7.887 | 1.569 | 10.727 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **13.305** | 12.781 | 0.737 | 13.822 | x1.49 |
| `scrapling [SQLite memory] (v2 relocate)` | **3916.847** | 3716.610 | 648.032 | 4726.324 | x439.97 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **44.208** | 39.341 | 6.002 | 49.314 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **54.651** | 49.642 | 16.942 | 156.558 | x1.24 |
| `oxide_parse [stream] sequential loop` | **106.252** | 91.174 | 51.118 | 149.077 | x2.40 |
| `oxide_parse [lite] sequential loop` | **117.837** | 102.704 | 36.523 | 148.117 | x2.67 |
| `oxide_parse [scraper] extract_many (Rayon)` | **289.370** | 237.513 | 139.477 | 411.414 | x6.55 |
| `selectolax [lexbor] sequential loop` | **510.615** | 382.369 | 93.386 | 752.052 | x11.55 |
| `oxide_parse [scraper] sequential loop` | **606.639** | 564.945 | 193.533 | 876.204 | x13.72 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.328** | 3.192 | 0.545 | 5.184 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **4.027** | 3.862 | 1.465 | 5.734 | x1.21 |
| `stream: localized ('1 299,50 ₽')` | **5.084** | 4.488 | 0.929 | 6.775 | x1.53 |
| `lite: localized ('1 299,50 ₽')` | **6.659** | 5.109 | 1.880 | 7.409 | x2.00 |
| `scraper: fast ASCII ('$19.99')` | **13.154** | 11.949 | 2.460 | 16.003 | x3.95 |
| `scraper: localized ('1 299,50 ₽')` | **14.693** | 13.688 | 1.579 | 16.394 | x4.42 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **4.856** | 4.282 | 2.274 | 6.593 | x1.00 |
| `lite: 2 selectors (run_native)` | **6.408** | 5.470 | 1.144 | 7.229 | x1.32 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **8.608** | 6.899 | 1.790 | 9.396 | x1.77 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **11.666** | 9.286 | 3.253 | 13.408 | x2.40 |
| `scraper: 2 selectors (run_native)` | **40.095** | 34.700 | 6.669 | 45.094 | x8.26 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **46.099** | 38.611 | 9.034 | 56.982 | x9.49 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
