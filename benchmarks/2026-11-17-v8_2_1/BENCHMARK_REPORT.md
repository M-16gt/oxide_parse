# Полный отчет бенчмарков `oxide_parse` — 2026-11-17

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
| 100 | False | 24.6 | 1.577 | **0.556** | 0.572 | 3.021 | 0.899 | 5.886 | 14.883 | 21.482 | 40.952 |
| 100 | True | 61.0 | 3.808 | 0.839 | **0.749** | 4.456 | 1.147 | 9.443 | 17.446 | 22.567 | 75.336 |
| 1000 | False | 249.4 | 17.274 | 6.277 | **4.871** | 27.244 | 8.144 | 70.612 | 143.093 | 195.611 | 407.165 |
| 1000 | True | 613.7 | 40.011 | 8.753 | **7.437** | 37.806 | 11.442 | 91.331 | 184.876 | 248.447 | 824.640 |
| 10000 | False | 2532.0 | 151.084 | 50.222 | **42.130** | 294.692 | 78.005 | 562.438 | 1396.794 | 2021.561 | 3651.894 |
| 10000 | True | 6174.6 | 376.326 | 91.444 | **74.462** | 447.263 | 143.319 | 1001.140 | 1872.341 | 2481.655 | 8869.278 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.584 | 1.542 | 0.041 |
| 100 | False | `lite` | 0.640 | 0.559 | 0.081 |
| 100 | False | `stream` | 0.617 | 0.508 | 0.109 |
| 100 | True | `scraper` | 4.019 | 4.090 | -0.071 |
| 100 | True | `lite` | 0.843 | 0.770 | 0.074 |
| 100 | True | `stream` | 0.756 | 0.758 | -0.003 |
| 1000 | False | `scraper` | 14.973 | 13.664 | 1.309 |
| 1000 | False | `lite` | 4.535 | 4.114 | 0.422 |
| 1000 | False | `stream` | 5.199 | 3.975 | 1.224 |
| 1000 | True | `scraper` | 36.916 | 35.889 | 1.027 |
| 1000 | True | `lite` | 8.250 | 7.640 | 0.610 |
| 1000 | True | `stream` | 6.229 | 5.674 | 0.555 |
| 10000 | False | `scraper` | 154.417 | 149.650 | 4.767 |
| 10000 | False | `lite` | 48.510 | 44.845 | 3.665 |
| 10000 | False | `stream` | 51.374 | 44.111 | 7.263 |
| 10000 | True | `scraper` | 414.907 | 377.969 | 36.938 |
| 10000 | True | `lite` | 86.109 | 82.299 | 3.811 |
| 10000 | True | `stream` | 74.865 | 63.063 | 11.802 |

## 2. Результаты по всем специальным сценариям
### Early-Exit Head + 1st Offer on 1591 KiB page
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.397** | 2.133 | 0.456 | 2.651 | x1.00 |
| `oxide_parse [lite]` | **9.202** | 8.274 | 1.593 | 10.757 | x3.84 |
| `selectolax [lexbor]` | **14.694** | 12.770 | 8.340 | 22.811 | x6.13 |
| `oxide_parse [scraper]` | **49.771** | 44.682 | 18.641 | 65.507 | x20.76 |
| `lxml [direct]` | **138.185** | 134.299 | 56.832 | 211.308 | x57.65 |
| `parsel` | **158.518** | 147.331 | 12.350 | 201.941 | x66.13 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.241** | 2.108 | 0.353 | 2.670 | x1.00 |
| `oxide_parse [stream]` | **2.439** | 2.303 | 0.176 | 3.161 | x1.09 |
| `selectolax [lexbor]` | **3.708** | 2.956 | 1.350 | 5.033 | x1.65 |
| `oxide_parse [scraper]` | **11.291** | 8.128 | 2.239 | 12.288 | x5.04 |
| `parsel` | **36.060** | 31.921 | 3.363 | 43.519 | x16.09 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **6.080** | 6.023 | 0.312 | 6.556 | x1.00 |
| `oxide_parse [scraper]` | **13.396** | 10.955 | 1.337 | 16.012 | x2.20 |
| `lxml [xpath]` | **14.304** | 9.780 | 4.532 | 17.542 | x2.35 |
| `parsel [xpath]` | **17.988** | 13.388 | 2.823 | 20.873 | x2.96 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.242** | 1.932 | 3.011 | 6.231 | x1.00 |
| `lxml + json.loads` | **4.946** | 4.810 | 0.372 | 5.858 | x2.21 |
| `oxide_parse [scraper]` | **5.666** | 4.747 | 2.558 | 8.057 | x2.53 |
| `oxide_parse [lite]` | **5.753** | 3.967 | 2.195 | 7.183 | x2.57 |
| `oxide_parse [stream]` | **6.470** | 4.893 | 1.545 | 7.118 | x2.89 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.584** | 1.964 | 0.817 | 3.556 | x1.00 |
| `selectolax [strip_tags]` | **3.326** | 2.910 | 0.730 | 4.210 | x1.29 |
| `oxide_parse [scraper]` | **11.513** | 8.349 | 1.648 | 12.137 | x4.46 |
| `bs4 + lxml [decompose]` | **154.397** | 128.090 | 40.527 | 172.084 | x59.75 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.439** | 0.314 | 0.122 | 1.390 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.570** | 1.365 | 0.329 | 1.766 | x3.58 |
| `scrapling [SQLite memory] (v1 match)` | **6.857** | 5.841 | 1.120 | 10.787 | x15.64 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.485** | 2.103 | 0.765 | 3.231 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **3.464** | 3.032 | 1.937 | 6.307 | x1.39 |
| `scrapling [SQLite memory] (v2 relocate)` | **612.143** | 560.450 | 107.002 | 696.309 | x246.33 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.097** | 0.939 | 0.684 | 1.739 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **6.113** | 4.548 | 3.221 | 9.751 | x5.57 |
| `scrapling [SQLite memory] (v1 match)` | **18.487** | 15.779 | 3.608 | 23.383 | x16.85 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **9.894** | 8.009 | 1.772 | 11.790 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **15.790** | 11.776 | 5.123 | 19.231 | x1.60 |
| `scrapling [SQLite memory] (v2 relocate)` | **4445.675** | 3957.614 | 285.697 | 4893.954 | x449.34 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **41.405** | 37.854 | 5.374 | 54.206 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **47.810** | 44.297 | 12.815 | 60.210 | x1.15 |
| `oxide_parse [stream] sequential loop` | **102.533** | 87.566 | 14.547 | 116.908 | x2.48 |
| `oxide_parse [lite] sequential loop` | **108.689** | 101.021 | 2.851 | 117.634 | x2.63 |
| `oxide_parse [scraper] extract_many (Rayon)` | **238.331** | 216.813 | 24.631 | 266.352 | x5.76 |
| `selectolax [lexbor] sequential loop` | **419.112** | 377.948 | 53.889 | 475.670 | x10.12 |
| `oxide_parse [scraper] sequential loop` | **586.052** | 551.914 | 30.094 | 607.345 | x14.15 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.576** | 2.603 | 1.249 | 3.962 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **4.664** | 3.138 | 0.620 | 6.019 | x1.30 |
| `stream: localized ('1 299,50 ₽')` | **4.865** | 3.693 | 0.709 | 5.224 | x1.36 |
| `lite: localized ('1 299,50 ₽')` | **5.737** | 4.423 | 0.976 | 6.862 | x1.60 |
| `scraper: fast ASCII ('$19.99')` | **12.810** | 8.120 | 3.009 | 13.977 | x3.58 |
| `scraper: localized ('1 299,50 ₽')` | **13.196** | 9.400 | 6.264 | 17.484 | x3.69 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **5.331** | 4.137 | 1.678 | 9.304 | x1.00 |
| `lite: 2 selectors (run_native)` | **6.565** | 5.123 | 1.240 | 6.952 | x1.23 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **7.441** | 6.420 | 5.612 | 12.951 | x1.40 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **9.066** | 8.030 | 1.186 | 11.665 | x1.70 |
| `scraper: 2 selectors (run_native)` | **39.164** | 31.137 | 14.981 | 54.746 | x7.35 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **54.423** | 44.676 | 14.857 | 66.696 | x10.21 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
