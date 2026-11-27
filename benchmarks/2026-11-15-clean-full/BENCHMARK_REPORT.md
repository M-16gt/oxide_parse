# Полный отчет бенчмарков `oxide_parse` — 2026-11-15

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
| 100 | False | 24.6 | 2.056 | 0.612 | **0.570** | 3.912 | 0.875 | 6.059 | 15.230 | 22.408 | 39.189 |
| 100 | True | 61.0 | 5.895 | 1.406 | **1.256** | 5.026 | 1.806 | 12.782 | 24.879 | 33.856 | 107.501 |
| 1000 | False | 249.4 | 20.085 | 7.314 | **6.738** | 33.087 | 9.304 | 79.979 | 150.438 | 305.096 | 423.429 |
| 1000 | True | 613.7 | 51.564 | 12.703 | **10.723** | 52.760 | 16.858 | 127.462 | 250.596 | 329.392 | 1134.960 |
| 10000 | False | 2532.0 | 214.882 | 75.507 | **60.906** | 406.256 | 100.435 | 791.292 | 1869.074 | 2771.206 | 4730.546 |
| 10000 | True | 6174.6 | 519.601 | 133.101 | **93.516** | 592.649 | 172.132 | 1290.716 | 2435.033 | 3341.344 | 11578.232 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.663 | 1.484 | 0.179 |
| 100 | False | `lite` | 0.716 | 0.566 | 0.150 |
| 100 | False | `stream` | 0.617 | 0.531 | 0.086 |
| 100 | True | `scraper` | 5.074 | 5.234 | -0.160 |
| 100 | True | `lite` | 1.204 | 1.111 | 0.092 |
| 100 | True | `stream` | 1.125 | 1.017 | 0.108 |
| 1000 | False | `scraper` | 18.896 | 19.051 | -0.155 |
| 1000 | False | `lite` | 6.383 | 6.208 | 0.175 |
| 1000 | False | `stream` | 5.705 | 4.437 | 1.267 |
| 1000 | True | `scraper` | 52.369 | 50.589 | 1.780 |
| 1000 | True | `lite` | 12.123 | 11.193 | 0.930 |
| 1000 | True | `stream` | 9.800 | 8.783 | 1.018 |
| 10000 | False | `scraper` | 220.433 | 199.071 | 21.362 |
| 10000 | False | `lite` | 74.446 | 63.190 | 11.256 |
| 10000 | False | `stream` | 60.528 | 52.901 | 7.627 |
| 10000 | True | `scraper` | 421.077 | 427.814 | -6.737 |
| 10000 | True | `lite` | 100.935 | 91.047 | 9.888 |
| 10000 | True | `stream` | 84.070 | 68.034 | 16.036 |

## 2. Результаты по всем специальным сценариям
### Early-Exit Head + 1st Offer on 1591 KiB page
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream]` | **2.905** | 2.229 | 1.150 | 4.188 | x1.00 |
| `oxide_parse [lite]` | **10.111** | 9.718 | 3.699 | 13.930 | x3.48 |
| `selectolax [lexbor]` | **15.946** | 13.377 | 11.136 | 28.682 | x5.49 |
| `oxide_parse [scraper]` | **62.730** | 48.462 | 36.625 | 121.833 | x21.60 |
| `lxml [direct]` | **146.699** | 125.951 | 115.092 | 306.543 | x50.50 |
| `parsel` | **182.888** | 146.357 | 122.062 | 289.512 | x62.96 |

### Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) (149.1 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **1.957** | 1.738 | 0.663 | 2.567 | x1.00 |
| `oxide_parse [stream]` | **2.270** | 1.840 | 0.574 | 4.201 | x1.16 |
| `selectolax [lexbor]` | **3.057** | 2.526 | 3.550 | 9.486 | x1.56 |
| `oxide_parse [scraper]` | **9.524** | 6.858 | 5.303 | 16.559 | x4.87 |
| `parsel` | **31.086** | 27.994 | 8.170 | 66.721 | x15.89 |

### XPath 1.0 (contains, following-sibling, parent, count on 600 rows) (123.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **6.139** | 4.874 | 2.463 | 19.706 | x1.00 |
| `oxide_parse [scraper]` | **11.913** | 10.289 | 6.318 | 18.978 | x1.94 |
| `lxml [xpath]` | **13.513** | 11.737 | 3.920 | 23.908 | x2.20 |
| `parsel [xpath]` | **14.520** | 12.617 | 8.098 | 28.026 | x2.37 |

### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.230** | 1.751 | 3.092 | 5.660 | x1.00 |
| `lxml + json.loads` | **4.775** | 4.433 | 1.606 | 6.477 | x2.14 |
| `oxide_parse [stream]` | **5.497** | 4.714 | 1.446 | 7.244 | x2.46 |
| `oxide_parse [lite]` | **5.569** | 5.357 | 2.175 | 7.847 | x2.50 |
| `oxide_parse [scraper]` | **6.599** | 4.922 | 2.108 | 12.293 | x2.96 |

### Dirty Text + Entities + Subtree Exclusion (600 blocks, 207.6 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite]` | **2.743** | 1.937 | 1.058 | 3.442 | x1.00 |
| `selectolax [strip_tags]` | **3.593** | 2.772 | 0.971 | 4.756 | x1.31 |
| `oxide_parse [scraper]` | **10.070** | 9.578 | 1.316 | 12.018 | x3.67 |
| `bs4 + lxml [decompose]` | **156.544** | 120.169 | 30.364 | 186.529 | x57.06 |

### Adaptive Hot Path (Selector Matches, 400 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **0.378** | 0.340 | 0.040 | 0.736 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **1.694** | 1.615 | 0.502 | 2.609 | x4.49 |
| `scrapling [SQLite memory] (v1 match)` | **7.983** | 6.428 | 4.245 | 20.475 | x21.15 |

### Adaptive Cold Path (Broken Selector Relocation across 400 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **2.641** | 2.053 | 0.922 | 3.788 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **4.304** | 2.957 | 2.174 | 5.951 | x1.63 |
| `scrapling [SQLite memory] (v2 relocate)` | **721.210** | 565.169 | 183.884 | 880.318 | x273.11 |

### Adaptive Hot Path (Selector Matches, 1600 filler nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v1 match)` | **1.095** | 0.920 | 0.205 | 1.242 | x1.00 |
| `oxide_parse [scraper] (v1 match)` | **5.412** | 4.884 | 1.732 | 11.125 | x4.94 |
| `scrapling [SQLite memory] (v1 match)` | **16.398** | 16.108 | 1.316 | 17.833 | x14.97 |

### Adaptive Cold Path (Broken Selector Relocation across 1600 nodes)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [lite] (v2 relocate)` | **8.870** | 7.880 | 1.897 | 13.838 | x1.00 |
| `oxide_parse [scraper] (v2 relocate)` | **13.582** | 12.994 | 2.424 | 15.553 | x1.53 |
| `scrapling [SQLite memory] (v2 relocate)` | **4648.734** | 4240.219 | 912.548 | 5589.710 | x524.13 |

### Batch 32 docs x 500 noisy products (9813 KiB total)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `oxide_parse [stream] extract_many (Rayon)` | **39.532** | 37.762 | 13.753 | 85.734 | x1.00 |
| `oxide_parse [lite] extract_many (Rayon)` | **55.168** | 49.755 | 5.191 | 63.339 | x1.40 |
| `oxide_parse [stream] sequential loop` | **106.260** | 93.863 | 22.387 | 159.946 | x2.69 |
| `oxide_parse [lite] sequential loop` | **139.582** | 118.380 | 21.468 | 187.355 | x3.53 |
| `oxide_parse [scraper] extract_many (Rayon)` | **250.623** | 243.817 | 28.084 | 284.556 | x6.34 |
| `selectolax [lexbor] sequential loop` | **419.475** | 397.824 | 40.332 | 600.221 | x10.61 |
| `oxide_parse [scraper] sequential loop` | **634.333** | 574.558 | 231.092 | 902.783 | x16.05 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.577** | 2.558 | 1.566 | 4.870 | x1.00 |
| `stream: localized ('1 299,50 ₽')` | **4.334** | 3.487 | 0.739 | 8.712 | x1.21 |
| `lite: fast ASCII ('$19.99')` | **4.369** | 3.614 | 0.909 | 8.022 | x1.22 |
| `lite: localized ('1 299,50 ₽')` | **5.280** | 3.938 | 2.098 | 7.074 | x1.48 |
| `scraper: fast ASCII ('$19.99')` | **10.178** | 8.843 | 2.129 | 13.511 | x2.85 |
| `scraper: localized ('1 299,50 ₽')` | **13.500** | 9.647 | 3.718 | 17.205 | x3.77 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **5.268** | 4.335 | 1.992 | 6.483 | x1.00 |
| `lite: 2 selectors (run_native)` | **6.390** | 5.314 | 1.462 | 7.469 | x1.21 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **8.347** | 7.476 | 2.591 | 11.604 | x1.58 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **11.912** | 9.232 | 2.760 | 13.227 | x2.26 |
| `scraper: 2 selectors (run_native)` | **37.289** | 34.929 | 7.346 | 43.955 | x7.08 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **49.753** | 40.584 | 7.529 | 50.242 | x9.44 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
