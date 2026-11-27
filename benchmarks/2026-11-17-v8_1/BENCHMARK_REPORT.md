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
| 100 | False | 24.6 | 2.271 | 0.707 | **0.625** | 3.799 | 1.010 | 8.924 | 19.913 | 31.568 | 49.613 |
| 100 | True | 61.0 | 4.824 | 1.149 | **1.054** | 4.854 | 1.474 | 11.790 | 22.261 | 29.202 | 99.450 |
| 1000 | False | 249.4 | 19.828 | 6.679 | **6.032** | 32.787 | 9.101 | 68.736 | 167.761 | 254.580 | 439.028 |
| 1000 | True | 613.7 | 44.794 | 9.780 | **8.726** | 47.156 | 14.424 | 109.509 | 210.489 | 283.248 | 961.990 |
| 10000 | False | 2532.0 | 191.353 | 62.236 | **54.492** | 363.078 | 92.426 | 720.074 | 1621.609 | 2380.443 | 4262.665 |
| 10000 | True | 6174.6 | 506.380 | 115.380 | **97.711** | 531.798 | 160.121 | 1152.197 | 2240.206 | 3025.813 | 10482.547 |

### Сопоставление extract и run_native

Разница медиан даёт приблизительную оценку накладных расходов Python. run_native дополнительно считает значения результата и освобождает Rust-результат. Отрицательная разница сохраняется и показывает вариативность измерений.

| Товаров | Noise | Движок | extract (мс) | run_native (мс) | Разница (мс) |
|---:|:---:|---|---:|---:|---:|
| 100 | False | `scraper` | 1.669 | 1.682 | -0.013 |
| 100 | False | `lite` | 0.625 | 0.831 | -0.206 |
| 100 | False | `stream` | 0.617 | 0.500 | 0.117 |
| 100 | True | `scraper` | 4.530 | 4.568 | -0.038 |
| 100 | True | `lite` | 0.984 | 0.968 | 0.016 |
| 100 | True | `stream` | 0.912 | 0.909 | 0.003 |
| 1000 | False | `scraper` | 18.784 | 17.384 | 1.400 |
| 1000 | False | `lite` | 5.759 | 4.833 | 0.926 |
| 1000 | False | `stream` | 4.965 | 4.282 | 0.683 |
| 1000 | True | `scraper` | 43.133 | 42.447 | 0.685 |
| 1000 | True | `lite` | 9.095 | 9.485 | -0.389 |
| 1000 | True | `stream` | 8.558 | 7.763 | 0.795 |
| 10000 | False | `scraper` | 186.415 | 187.382 | -0.967 |
| 10000 | False | `lite` | 60.021 | 54.760 | 5.261 |
| 10000 | False | `stream` | 50.701 | 45.614 | 5.087 |
| 10000 | True | `scraper` | 492.756 | 493.075 | -0.319 |
| 10000 | True | `lite` | 100.927 | 95.103 | 5.824 |
| 10000 | True | `stream` | 135.464 | 124.010 | 11.455 |

## 2. Результаты по всем специальным сценариям
### Hybrid HTML + __NEXT_DATA__ JSON (1000 items, 124.9 KiB)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `selectolax + json.loads` | **2.166** | 1.970 | 2.369 | 6.476 | x1.00 |
| `oxide_parse [lite]` | **4.830** | 3.609 | 2.395 | 6.999 | x2.23 |
| `lxml + json.loads` | **5.655** | 5.409 | 0.739 | 6.611 | x2.61 |
| `oxide_parse [stream]` | **5.907** | 5.119 | 2.592 | 8.142 | x2.73 |
| `oxide_parse [scraper]` | **6.016** | 5.281 | 2.663 | 9.255 | x2.78 |

### Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: fast ASCII ('$19.99')` | **3.560** | 3.176 | 0.807 | 4.268 | x1.00 |
| `lite: fast ASCII ('$19.99')` | **3.989** | 3.783 | 0.712 | 4.755 | x1.12 |
| `stream: localized ('1 299,50 ₽')` | **5.310** | 3.683 | 0.700 | 6.427 | x1.49 |
| `lite: localized ('1 299,50 ₽')` | **5.543** | 4.990 | 0.638 | 8.586 | x1.56 |
| `scraper: fast ASCII ('$19.99')` | **11.104** | 10.303 | 1.174 | 12.869 | x3.12 |
| `scraper: localized ('1 299,50 ₽')` | **13.418** | 12.331 | 4.232 | 21.300 | x3.77 |

### Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)
| Реализация | Медиана (мс) | Min (мс) | IQR (мс) | Max (мс) | Отставание от лидера |
|---|---:|---:|---:|---:|---:|
| `stream: 2 selectors (run_native)` | **5.845** | 4.848 | 2.422 | 28.518 | x1.00 |
| `lite: 2 selectors (run_native)` | **7.269** | 6.761 | 3.477 | 13.746 | x1.24 |
| `stream: 11 selectors incl. 5 misses (run_native)` | **9.566** | 8.208 | 3.895 | 14.056 | x1.64 |
| `lite: 11 selectors incl. 5 misses (run_native)` | **15.259** | 10.315 | 6.646 | 37.596 | x2.61 |
| `scraper: 2 selectors (run_native)` | **49.751** | 38.632 | 23.192 | 65.087 | x8.51 |
| `scraper: 11 selectors incl. 5 misses (run_native)` | **61.500** | 47.562 | 17.280 | 95.204 | x10.52 |


## Пропущенные реализации

- `selectolax [modest]` — Modest backend is deprecated since selectolax 1.0. The engine has not been updated since 2021, while lexbor has been available since 2021 and is actively developed. Modest is outdated, not maintained, contains bugs and does not follow modern HTML5 standards. Please use lexbor backend instead: `from selectolax.lexbor import LexborHTMLParser`. The APIs are nearly identical, so in most projects swapping the import is all that's needed.

Один прогрев, случайный порядок реализаций в каждом раунде, GC перед таймером. Каждый измеренный результат проверяется. Пауза cooldown выполняется между группами сценариев. Размеры представлены в байтах UTF-8.

Нагрузка ОС, температура и частоты CPU не фиксировались. Quick предназначен для проверки работоспособности. Сравнение времени относится к конкретным схемам и размерам этого прогона.
