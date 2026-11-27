# Полный отчет бенчмарков `oxide_parse` — 2026-11-16

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
| 100 | False | 24.6 | 2.155 | 0.742 | **0.648** | 4.270 | 0.992 | 7.926 | 20.245 | 28.302 | 47.363 |
| 100 | True | 61.0 | 4.505 | 1.325 | **1.078** | 5.259 | 1.644 | 12.592 | 25.997 | 33.238 | 117.584 |
| 1000 | False | 249.4 | 20.970 | 7.133 | **5.707** | 36.849 | 10.228 | 82.485 | 170.238 | 285.101 | 494.272 |
| 1000 | True | 613.7 | 46.923 | 10.231 | **8.611** | 42.570 | 14.064 | 104.422 | 199.486 | 311.962 | 963.938 |
| 10000 | False | 2532.0 | 197.904 | 66.074 | **53.724** | 372.573 | 99.027 | 725.086 | 1744.732 | 2632.440 | 4370.116 |
| 10000 | True | 6174.6 | 499.385 | 121.737 | **94.208** | 573.285 | 173.474 | 1243.807 | 2405.628 | 3255.542 | 11344.352 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 5.962 | 12.930 | -6.968 |
| 100 | False | `lite` | 0.703 | 0.645 | 0.058 |
| 100 | False | `stream` | 0.585 | 0.515 | 0.069 |
| 100 | True | `scraper` | 6.005 | 5.756 | 0.248 |
| 100 | True | `lite` | 1.044 | 0.950 | 0.094 |
| 100 | True | `stream` | 0.898 | 0.848 | 0.050 |
| 1000 | False | `scraper` | 17.675 | 18.413 | -0.738 |
| 1000 | False | `lite` | 6.492 | 6.061 | 0.432 |
| 1000 | False | `stream` | 5.573 | 4.097 | 1.477 |
| 1000 | True | `scraper` | 56.561 | 49.522 | 7.039 |
| 1000 | True | `lite` | 12.006 | 11.038 | 0.969 |
| 1000 | True | `stream` | 8.464 | 8.095 | 0.369 |
| 10000 | False | `scraper` | 198.391 | 197.012 | 1.379 |
| 10000 | False | `lite` | 73.702 | 70.564 | 3.138 |
| 10000 | False | `stream` | 55.772 | 49.939 | 5.833 |
| 10000 | True | `scraper` | 495.260 | 474.438 | 20.822 |
| 10000 | True | `lite` | 116.407 | 114.273 | 2.134 |
| 10000 | True | `stream` | 95.260 | 87.864 | 7.396 |

## 2. Результаты по всем специальным сценариям
### Early-Exit Head + 1st Offer on 1591 KiB page
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **3.292** | 2.811 | 0.352 | 220.835 | x1.00 |
| `oxide_parse [lite]` | **12.074** | 11.252 | 1.138 | 13.126 | x3.67 |
| `selectolax [lexbor]` | **19.665** | 16.527 | 2.330 | 20.279 | x5.97 |
| `oxide_parse [scraper]` | **65.733** | 53.657 | 12.119 | 1040.766 | x19.97 |
| `lxml [direct]` | **163.012** | 161.425 | 25.594 | 274.361 | x49.52 |
| `parsel` | **197.312** | 192.959 | 24.644 | 715.813 | x59.94 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.277** | 2.026 | 0.530 | 2.843 | x1.00 |
| `oxide_parse [stream]` | **2.889** | 2.111 | 0.771 | 4.130 | x1.27 |
| `selectolax [lexbor]` | **3.922** | 3.281 | 0.518 | 3.993 | x1.72 |
| `oxide_parse [scraper]` | **10.619** | 9.868 | 2.092 | 12.478 | x4.66 |
| `parsel` | **41.623** | 35.438 | 2.722 | 61.394 | x18.28 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **6.497** | 6.003 | 0.724 | 6.996 | x1.00 |
| `oxide_parse [scraper]` | **12.408** | 11.119 | 1.369 | 13.357 | x1.91 |
| `lxml [xpath]` | **16.074** | 13.063 | 4.725 | 30.717 | x2.47 |
| `parsel [xpath]` | **19.974** | 17.021 | 5.783 | 29.578 | x3.07 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.451** | 1.985 | 0.474 | 3.750 | x1.00 |
| `oxide_parse [lite]` | **4.337** | 3.943 | 0.901 | 10.748 | x1.77 |
| `lxml + json.loads` | **5.173** | 4.986 | 0.986 | 6.045 | x2.11 |
| `oxide_parse [scraper]` | **5.618** | 5.526 | 0.385 | 7.408 | x2.29 |
| `oxide_parse [stream]` | **6.048** | 5.404 | 1.789 | 12.357 | x2.47 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **3.015** | 2.511 | 0.412 | 3.645 | x1.00 |
| `selectolax [strip_tags]` | **3.666** | 2.913 | 0.162 | 4.038 | x1.22 |
| `oxide_parse [scraper]` | **12.593** | 11.083 | 1.163 | 14.585 | x4.18 |
| `bs4 + lxml [decompose]` | **175.966** | 161.625 | 22.775 | 195.923 | x58.37 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.425** | 0.277 | 0.102 | 0.471 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.879** | 1.600 | 0.377 | 2.859 | x4.42 |
| `scrapling [SQLite memory] (v1 match)` | **6.045** | 5.245 | 1.676 | 7.821 | x14.21 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **3.311** | 2.937 | 0.630 | 4.782 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **4.597** | 3.804 | 5.507 | 11.627 | x1.39 |
| `scrapling [SQLite memory] (v2 relocate)` | **794.142** | 722.369 | 49.019 | 2575.685 | x239.87 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.397** | 1.361 | 0.104 | 1.659 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **7.655** | 6.332 | 0.365 | 7.869 | x5.48 |
| `scrapling [SQLite memory] (v1 match)` | **24.880** | 22.891 | 3.778 | 33.039 | x17.81 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **12.151** | 10.275 | 2.278 | 17.502 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **17.382** | 15.383 | 1.360 | 18.936 | x1.43 |
| `scrapling [SQLite memory] (v2 relocate)` | **5422.258** | 5242.710 | 591.680 | 5969.142 | x446.25 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **51.606** | 41.541 | 14.755 | 64.171 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **57.860** | 52.008 | 22.950 | 128.273 | x1.12 |
| `oxide_parse [stream] sequential loop` | **121.670** | 88.521 | 32.027 | 1809.389 | x2.36 |
| `oxide_parse [lite] sequential loop` | **148.653** | 117.132 | 61.866 | 190.179 | x2.88 |
| `oxide_parse [scraper] extract_many (Rayon)` | **260.292** | 238.879 | 25.258 | 359.099 | x5.04 |
| `selectolax [lexbor] sequential loop` | **487.745** | 428.951 | 70.875 | 670.173 | x9.45 |
| `oxide_parse [scraper] sequential loop` | **685.281** | 559.651 | 102.683 | 884.241 | x13.28 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.386** | 2.743 | 0.572 | 4.357 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **4.752** | 3.580 | 0.604 | 5.260 | x1.40 |
| `stream: localized ('1 299,50 ₽')` | **5.520** | 3.820 | 1.440 | 13.334 | x1.63 |
| `lite: localized ('1 299,50 ₽')` | **6.054** | 4.473 | 3.197 | 11.423 | x1.79 |
| `scraper: fast ASCII ('$19.99')` | **11.499** | 8.740 | 2.799 | 13.816 | x3.40 |
| `scraper: localized ('1 299,50 ₽')` | **12.514** | 10.883 | 2.718 | 15.368 | x3.70 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **5.573** | 5.272 | 0.590 | 7.528 | x1.00 |
| `lite: 2 selectors (run_native)` | **6.621** | 6.128 | 13.845 | 28.041 | x1.19 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **9.016** | 8.416 | 9.019 | 18.931 | x1.62 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **11.644** | 10.903 | 6.145 | 41.393 | x2.09 |
| `scraper: 2 selectors (run_native)` | **38.513** | 37.465 | 1.677 | 149.712 | x6.91 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **47.466** | 42.921 | 18.740 | 242.733 | x8.52 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
