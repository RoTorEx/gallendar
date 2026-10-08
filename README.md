# gallendar

Личный self-hosted сервис на Rust для синхронизации занятости между четырьмя
Google аккаунтами. Событие в одном аккаунте должно блокировать тот же интервал
в остальных трёх обезличенными событиями `Busy`.

## Текущий этап

Проверка гипотезы о вторичных календарях копий. Сервис ещё не реализован,
доступ к аккаунтам не настроен. Первый эксперимент — на двух личных аккаунтах;
затем требуется проверить все четыре целевых аккаунта и рабочее планирование.

Начать с очереди в [TASK.md](TASK.md) и ручного протокола в
[business/validation.md](business/validation.md). До подтверждения доступности
и sharing полный сервис не разрабатывать.

## Проверка документов

```bash
make check
```

Сейчас команда проверяет ошибки whitespace в Git diff. Это не тест календарей
или сервиса. Проверки Rust будут добавлены вместе с реализацией.

## Project truth

- `BUSINESS.md` — purpose, concepts, invariants, decisions, and business-module map.
- `business/*` — focused business domains, created only when needed.
- `CHANGELOG.md` — notable unreleased and released behavior.
- `AGENTS.md` — context router for coding agents.
- [TASK.md](TASK.md) — текущие проверки и отложенные неоднозначности.
- [ROADMAP.md](ROADMAP.md) — этапы разработки и условия перехода.

## Kernel

Refresh the committed operating rules explicitly:

```bash
make vibe-pull
```

If the parent path moved:

```bash
make vibe-kernel-set KERNEL=/absolute/path/to/vibecoding-kernel
make vibe-pull
```
