# gallendar

CLI на Rust для зеркала занятости **Aveni & Thira → Innowise**.
Оригиналы остаются в двух источниках, в Innowise появляются копии занятости с настроенным названием.
Переносы и удаления отражаются в копиях; обратных записей нет.

Маршруты задаются в `~/.x-cli-gallendar/config.toml`. Выполнить `gallendar setup`
для нового шаблона или `gallendar config --edit` для существующего.
Setup сохраняет существующий конфиг; новый шаблон — [gallendar.example.toml](gallendar.example.toml).
Aveni и Thira задают `source` и `owner_email`, Innowise — `target` и `owner_email`.
Точные ID, доступ и тип назначения ещё предстоит настроить и проверить.

```toml
[[routes]]
from = "aveni"
to = "innowise"
mode = "masked"
title = "Busy"

[[routes]]
from = "thira"
to = "innowise"
mode = "masked"
title = "Busy"
```

`mode = "masked"` отображает значение `title`, например `Busy`, `Занят` или
`Aveni — занят`. `mode = "original"` сохраняет название исходной встречи;
`title` в этом режиме игнорируется. Описание, участники, место и ссылки
не переносятся. Копии сохраняют Google visibility = private; видимость
названия зависит от прав наблюдателя. Изменение режима или title обновляет
существующие копии на следующем проходе. Без mode/title используются masked/Busy.

`from` и `to` ссылаются на `name` в `[[accounts]]`. Читаются только источники
маршрутов; копии управляются только в их назначениях. Пустой список маршрутов,
неизвестные имена, дубли и маршруты в себя отклоняются. Без `routes` старые
конфиги сохраняют прежнее поведение «все со всеми»; для текущего scope задайте
оба маршрута явно. Если удалить маршрут, его копии удаляются при следующем
успешном проходе, пока назначение остаётся в другом маршруте. Если назначение
полностью исключено, оно больше не читается и старые копии в нём остаются;
их очистку следует планировать отдельно. Namespace сохраняйте стабильным.

## Установка из GitHub

```bash
curl -fsSL https://github.com/RoTorEx/gallendar/releases/latest/download/gallendar-install.sh -o /tmp/gallendar-install.sh
sh /tmp/gallendar-install.sh
rm /tmp/gallendar-install.sh
gallendar version
gallendar update
```

Installer сохраняет существующий конфиг и проверяет SHA-256 и версию бинарника.

## Быстрый запуск

```bash
make build-local
./gallendar demo
make run ARGS=demo
make install-local
gallendar --help
gallendar version
gallendar demo
```

`demo` работает без ключей и сети: четыре синтетических источника дают 12 копий,
повторный проход — ноль изменений, перенос — три обновления, удаление — три
удаления. Проверяется приватность и остановка до записи при ошибке чтения.
Это демонстрация движка, а не подтверждение поведения Google.

`make install-local` устанавливает бинарник в `~/.x-cli-gallendar/bin`, сохраняет
существующий конфиг и добавляет PATH в профиль shell, как соседние CLI.
Для уже открытого терминала: `export PATH="$HOME/.x-cli-gallendar/bin:$PATH"`.

## Базовые команды

- `setup` (алиасы `init`, `setup-config`) — создаёт шаблон и в интерактивном терминале открывает
  его в редакторе; `setup --no-edit` подходит для скриптов. Существующий конфиг
  сохраняется. `EDITOR`/`VISUAL` задаёт путь к исполняемому файлу редактора.
- `config` — путь к конфигу; `config --edit` открывает его в редакторе.
- `home` — домашний каталог приложения `~/.x-cli-gallendar`.
- `version`, `-V`, `--version` — установленная версия; `--help` — справка.
- `update` — обновление текущего бинарника из последнего публичного
  [GitHub Release](https://github.com/RoTorEx/gallendar/releases) с проверкой
  SHA-256 и версии, атомарной заменой и запретом понижения версии.
  Конфиг и ключи сохраняются. `--repo OWNER/REPO` и `GALLENDAR_RELEASE_REPO`
  позволяют явно выбрать другой репозиторий.

Автообновление включено через `auto_update = true` в корне TOML (значение
по умолчанию для старых конфигов тоже true). При запуске doctor, sync, run
или availability с валидным конфигом установленный CLI запускает фоновую
проверку не чаще раза в 24 часа через публичную страницу последнего релиза,
без GitHub API-токена. Проверяется номер релиза; архив скачивается
только при новой версии. Ошибки не мешают основной команде и сохраняются в
`~/.x-cli-gallendar/update.log`; попытка учитывается и при ошибке, чтобы
не повторять сетевой запрос на каждом запуске. `auto_update = false` отключает
это поведение. Справка, version/home/config/setup и offline demo не обращаются
к сети. Локальные бинарники разработки не обновляются автоматически.
Новая версия используется со следующего запуска; уже работающий run продолжает
текущую версию. Автообновление не создаёт системный сервис или расписание.


Базовый интерфейс и доставка согласованы с соседями `linkdeck`, `hop`, `shelve`.

## Авторизация: сервисный ключ или OAuth

Оба способа поддерживаются. Старый конфиг без `auth` использует сервисный ключ;
существующий `service_account_key` сохраняется. В корне TOML выберите:

```toml
auth = "service_account"
service_account_key = "secrets/service-account.json"
```

Этот режим требует sharing источников с чтением деталей и назначения с записью.
В проверенных настройках Aveni и Innowise внешний sharing ограничен free/busy:
сервисный ключ в таком случае не подходит для текущего чтения событий и записи
копий. Ключ успешно проверен, но доступ к календарям не подтверждён.

Для пользовательского OAuth создайте в том же Cloud-проекте Google Auth platform:
[Branding](https://console.cloud.google.com/auth/branding) → Get started,
аудитория External; [Audience](https://console.cloud.google.com/auth/audience) →
Test users для первого входа или In production для регулярной эксплуатации.
В [Clients](https://console.cloud.google.com/auth/clients) выберите Create client →
Desktop app и скачайте JSON. Это другой файл, чем ключ service account.
Храните его в `~/.x-cli-gallendar/secrets/oauth-client.json` с правами 0600.
Доступ к API может потребовать одобрения администратора рабочей организации.

В существующем конфиге измените корневой `auth` и добавьте `[oauth]` перед
первым `[[accounts]]`. Остальные корневые параметры должны оставаться выше `[oauth]`:

```toml
auth = "oauth"
# Остальные корневые параметры, включая service_account_key, namespace и horizon_days.

[oauth]
client = "secrets/oauth-client.json"
tokens_dir = "secrets/oauth"

# Далее существующие [[accounts]] и [[routes]].
```

Относительные пути считаются от каталога config.toml. Заполните реальные
`owner_email` для трёх аккаунтов и сохраните явные маршруты. Вход возможен до
заполнения Calendar ID; doctor/sync/run требуют валидных ID.

```bash
gallendar auth login --account aveni
gallendar auth login --account thira
gallendar auth login --account innowise
gallendar auth status
gallendar doctor
gallendar sync --dry-run
```

В каждом открывшемся браузере выбирайте соответствующий рабочий аккаунт.
CLI проверяет email и полученные права перед сохранением. Источники запрашивают
чтение событий, назначения — изменение событий; дополнительно нужны free/busy
и проверка email. OAuth использует PKCE, state и локальный callback на 127.0.0.1.
`--no-browser` печатает ссылку без открытия браузера; её нужно открыть на том же
компьютере. Ожидание входа ограничено 180 секундами. Отмена, неправильный аккаунт
или недостаточные права сохраняют прежние локальные токены.

Для смешанного доступа задайте `auth = "oauth"` или `auth = "service_account"`
внутри нужного `[[accounts]]`: это переопределяет корневой режим только для него.
`auth status` проверяет наличие и соответствие локальных credentials; это не
проверка действительности Google-доступа. Секреты он не выводит и к сети не обращается.

Каждый аккаунт хранится в отдельном файле `tokens_dir/<name>.json` (0600),
каталог — 0700. Refresh token используется для получения нового access token на
каждом проходе; браузер повторно не открывается. Вход в другой аккаунт сохраняет
остальные токены. В режиме Testing Google ограничивает Calendar refresh token
семью днями; In production убирает этот срок, но отзыв доступа и корпоративные
ограничения могут потребовать повторного входа.
[Правила срока действия Google](https://developers.google.com/identity/protocols/oauth2#expiration).

Для переноса на сервер выполните вход на Mac и передайте по SSH OAuth client,
каталог токенов и TOML, сохранив права и относительные пути. Не запускайте два
писателя одного namespace одновременно. Готовые серверные сборки пока отложены;
текущая платформа доставки — Apple Silicon. Автозапуска нет: run работает, пока
жив процесс. После проверки реальной доступности назначения:

```bash
gallendar run --apply --availability-confirmed
```

Добавление участников не входит в эту реализацию: оба режима по-прежнему создают
приватные Busy-копии без гостей и без Google Meet ссылок.

## Справочная инструкция Google для прежнего эксперимента

Эта инструкция сохранена для справки о настройке Google Cloud и sharing.
Её двусторонний TOML и личный эксперимент относятся к прежнему scope.
Для текущего сценария используйте новый шаблон выше: чтение Aveni/Thira,
запись только Innowise и проверку [business/validation.md](business/validation.md).

Начинаем с двух личных Google аккаунтов A и B. Если второго личного аккаунта
пока нет, он понадобится для проверки доступности со стороны другого пользователя.
Рабочие аккаунты добавляются после первого эксперимента. Внешний доступ пока
не настроен; команды ниже выполняются владельцем в своих аккаунтах.

### 1. Проверить локальный бинарник

В каталоге проекта:

```bash
make build-local
./gallendar --version
./gallendar demo
```

Это оптимизированная сборка для текущего Mac, без релизного коммита, тега или
публикации. Файл `./gallendar` не включается в Git. Для установки в PATH:

```bash
make install-local
export PATH="$HOME/.x-cli-gallendar/bin:$PATH"
gallendar --version
```

После изменения исходников `make build-local` обновляет `./gallendar`, а
`make install-local` — установленную копию. Далее команды используют `gallendar`;
вместо него можно писать `./gallendar` из проекта. Конфиг у обеих копий общий.

### 2. Создать Google Cloud проект и включить API

Открыть [Google Cloud Console](https://console.cloud.google.com/) под личным
аккаунтом A. В переключателе проектов выбрать New project, задать имя, например
`gallendar-personal`, создать проект и убедиться, что он выбран.

В APIs & Services → Library найти **Google Calendar API** и нажать Enable.
[Страница API](https://console.cloud.google.com/apis/library/calendar-json.googleapis.com).
Сам CLI работает локально; создавать VM или Cloud Run для этого шага не нужно.
[Документация включения API](https://developers.google.com/workspace/calendar/api/quickstart/python#enable_the_api).

### 3. Создать сервисную учётную запись и скачать JSON

Открыть IAM & Admin → Service Accounts → Create service account.
Задать имя, например `gallendar-sync`, и завершить создание. Дополнительные
роли доступа к Cloud-проекту на этом этапе оставить пустыми: доступ к событиям
выдаётся в Google Calendar. Скопировать email новой учётной записи вида
`gallendar-sync@PROJECT_ID.iam.gserviceaccount.com`.
[Создание service account](https://docs.cloud.google.com/iam/docs/service-accounts-create).

Открыть эту учётную запись → Keys → Add key → Create new key → JSON → Create.
Браузер скачает JSON-файл. Он содержит приватный ключ: оставить его локально,
не отправлять в чат и не включать в Git.
[Создание JSON-ключа](https://docs.cloud.google.com/iam/docs/keys-create-delete).

Если создание service account или ключа запрещено политикой организации,
сохранить текст ошибки и остановить этот шаг. Это конкретный блокер;
не менять корпоративные ограничения ради продолжения инструкции.

### 4. Положить ключ в каталог приложения

В терминале, заменив имя файла на фактически скачанное:

```bash
mkdir -p "$HOME/.x-cli-gallendar/secrets"
chmod 700 "$HOME/.x-cli-gallendar/secrets"
mv -n "$HOME/Downloads/ИМЯ_СКАЧАННОГО_КЛЮЧА.json" \
  "$HOME/.x-cli-gallendar/secrets/service-account.json"
chmod 600 "$HOME/.x-cli-gallendar/secrets/service-account.json"
```

`mv -n` сохраняет уже существующий ключ. Если он есть, убедиться, что выбран
нужный service account, прежде чем использовать другой ключ. Путь в конфиге
можно указать абсолютный; символ `~` в TOML не раскрывается. Путь
`secrets/service-account.json` считается от каталога `config.toml`.

### 5. Подготовить календари в обоих аккаунтах

Открыть [Google Calendar](https://calendar.google.com/) в браузере отдельно
под A и B. В каждом аккаунте слева у Other calendars нажать `+` → Create new
calendar. Назвать календарь, например `gallendar Busy — A` / `gallendar Busy — B`.
Создавать его нужно от лица владельца аккаунта, чтобы календарь принадлежал ему.
[Создание календаря](https://support.google.com/calendar/answer/37095).

Затем в каждом аккаунте открыть Settings and sharing нужного календаря →
Shared with / Share with specific people or groups → Add people and groups.
Добавить **email service account** из шага 3 и выдать:

| Календарь | Разрешение сервисному аккаунту |
|---|---|
| Основной календарь с оригиналами | See all event details — чтение событий |
| Новый календарь копий | Make changes to events — изменение событий |

Повторить для A и B. Service account не требует ручного входа в Gmail;
в CLI используется скачанный ключ. Проверка фактического доступа — шаг 7.
[Разрешения sharing](https://support.google.com/calendar/answer/37082?hl=en).

В настройках каждого из четырёх календарей открыть **Integrate calendar →
Calendar ID** и скопировать ID. Основной ID часто равен email владельца;
ID вторичного календаря обычно заканчивается на `@group.calendar.google.com`.
Использовать именно поле Calendar ID, а не iCal URL или ссылку для браузера.
[Где найти Calendar ID](https://support.google.com/calendar/answer/44105?hl=en-GB).

Для эксперимента дополнительно поделиться основным календарём A с аккаунтом B,
а основным B — с A с разрешением **See only free/busy**. Это права наблюдателей,
отдельные от прав service account. Они нужны для сравнения доступности гостей.
[Проверка доступности другого пользователя](https://support.google.com/calendar/answer/6294878).

### 6. Заполнить локальный конфиг

```bash
gallendar setup --no-edit
gallendar config
nano "$HOME/.x-cli-gallendar/config.toml"
```

Если конфиг уже существует, setup его сохраняет. Заполнить его по образцу,
заменив все значения с `REPLACE` на реальные email и Calendar ID:

```toml
service_account_key = "secrets/service-account.json"
namespace = "gallendar-personal"
horizon_days = 30
poll_seconds = 60
all_day_policy = "out_of_office_only"

[[accounts]]
name = "personal-a"
owner_email = "REPLACE-A@gmail.com"
source = "REPLACE-A@gmail.com"
target = "REPLACE-A@group.calendar.google.com"

[[accounts]]
name = "personal-b"
owner_email = "REPLACE-B@gmail.com"
source = "REPLACE-B@gmail.com"
target = "REPLACE-B@group.calendar.google.com"
```

owner_email — email человека, а не сервисного аккаунта. source — календарь
оригиналов этого человека; target — его отдельный календарь копий. Не подставлять
`primary`: для service account он обозначает другой календарь.

Для первого теста подходят обычные события с началом и концом по часам.
`out_of_office_only` пропускает обычные события на весь день и оставляет all-day
только типа Out of Office. Если нужно переносить обычные Busy all-day,
оператор явно выбирает `busy`; типы birthday и workingLocation всё равно
исключаются. Значения 30 дней и 60 секунд — редактируемые параметры примера.
Namespace после начала синхронизации сохранять стабильным.

По умолчанию конфиг находится в `~/.x-cli-gallendar/config.toml`.
`GALLENDAR_CONFIG` задаёт другой путь, а `--config PATH` имеет приоритет над ним.

### 7. Проверить интеграцию без записи

```bash
gallendar doctor
gallendar sync --dry-run
```

Ожидается `ready: true`, source_readable и target_writable для обоих аккаунтов.
Для источника допустим reader, для назначения — writer или owner
(поддерживается также writerWithoutPrivateAccess). doctor проверяет роли,
но пока не создаёт событий и не доказывает доступность аккаунта.

Dry-run показывает create/update/delete/unchanged. Это события обоих источников
на весь выбранный горизонт, поэтому create может быть больше одного.
До записи проверить, что настроены правильные исходные календари и назначения.
Содержание оригиналов в вывод не попадает.

Типичные причины ошибок:

| Результат | Что проверить |
|---|---|
| Ошибка конфига / placeholders | Заполнены ли все REPLACE, email, явные Calendar ID и обязательные поля |
| Cannot read service-account key | Файл существует, путь считается от конфига, TOML не использует `~` |
| Invalid service-account JSON | Скачан JSON именно сервисного аккаунта |
| Ошибка авторизации | Ключ действующий, service account включён, системные часы корректны |
| HTTP 403 при чтении событий | Calendar API включён в проекте ключа; source/target выданы правильные права |
| HTTP 404 | Calendar ID правильный; календарь расшарен нужному service account |
| ready: false | Источник имеет чтение событий, а target — изменение событий |
| Another gallendar process | Остановить run для этого конфига перед отдельным sync |

При неизвестной ошибке сохранить вывод команды без содержания JSON-ключа;
готовность интеграции не выводить только из настроек UI.

### 8. Проверить гипотезу о доступности

Выбрать свободный будущий интервал в пределах горизонта. Создать черновик
события в A, добавить B как гостя и открыть Find a time; приглашение не отправлять.
Проверять именно строку B, а не общую сетку календарей.

Сначала создать временный Busy в основном календаре B: A должен увидеть B
занятым. Удалить контрольное событие и убедиться, что B снова свободен.
Затем создать такой же временный `Busy` только в календаре копий B и повторить
наблюдение из A. Проверить скрытие календаря копий, перенос и удаление блока;
повторить в обратном направлении. Используемые страницы бронирования проверяются
отдельно. Полный протокол: [business/validation.md](business/validation.md).

Если B остаётся свободным, это NO-GO вторичных календарей: авторизация может
работать, но основной продуктовый результат не достигнут. Не подтверждать
availability-confirmed; сначала пересмотреть назначение Busy-копий.

Перед синхронизацией убрать ручные тестовые блоки. CLI не управляет вручную
созданными событиями без его приватных меток. Успех этой проверки на личных
аккаунтах не заменяет проверку рабочих аккаунтов.

### 9. Проверить запись и включить периодическую работу

Только после GO создать обычное Busy-событие в источнике A, без гостей,
на свободный будущий интервал. Посмотреть dry-run и затем применить:

```bash
gallendar sync --dry-run
gallendar sync --apply --availability-confirmed
```

В календаре копий B должен появиться `Busy`; источник сохраняет оригинал.
CLI подтверждает read-back; дополнительно посмотреть событие в Google Calendar
и доступность B из другого аккаунта. Повторный sync не создаёт дубликата.
Перенести оригинал, повторить sync и проверить перенос копии; удалить оригинал,
повторить sync и проверить исчезновение его копии. Повторить B → A.

После этого запустить в терминале:

```bash
gallendar run --apply --availability-confirmed
```

Процесс работает, пока открыт и запущен; Ctrl-C останавливает его. Для фоновой
работы после закрытия терминала понадобится отдельная настройка запуска на Mac
или VPS — локальный foreground run сам этого не обеспечивает.

Диагностическая команда availability требует --account NAME и --start/--end
в RFC3339 с явным UTC offset. Для пользовательских времён использовать
Europe/Warsaw и смещение для выбранной даты. Она отдельно показывает busy
источника и копий, не доказывая суммарную доступность аккаунта.

`--json` даёт машинный JSON для диагностики и путей/версии; run выдаёт по одному
JSON на строку. update и открытие редактора не поддерживают этот флаг.

## Синхронизация и проверка гипотезы

Ручной эксперимент: [business/validation.md](business/validation.md).
Текущие проверки и неоднозначности: [TASK.md](TASK.md).
После внешней проверки доступности доступны команды записи:

```bash
gallendar sync --apply --availability-confirmed
gallendar run --apply --availability-confirmed
```

Флаг подтверждает выполненную оператором проверку; CLI не может доказать
поведение «Найти время» за коллегу. Без `--apply` оба режима только читают и
показывают количества изменений. `run` запускает проходы с заданным интервалом,
ошибки пишет в stderr и повторяет следующий проход; Ctrl-C завершает процесс.
Ключ авторизации обновляется в начале каждого прохода.

`availability --account NAME --start RFC3339 --end RFC3339` отдельно показывает
занятость источника и календаря копий. Часы требуют явного смещения UTC;
суммарная доступность аккаунта этим запросом не подтверждается.

Движок раскрывает повторения через Google API, учитывает переносы и отмены
вхождений, не копирует Free, отклонённые владельцем приглашения, birthday и
workingLocation. Tentative события и ответ владельца «возможно» допускаются,
если событие не Free. Для all-day требуется явная политика:
`out_of_office_only` или `busy` (второй вариант включает обычные Busy all-day).
All-day копии представлены временным интервалом от полуночи до полуночи в зоне
источника, чтобы разные зоны календарей назначения не сдвигали занятость.

Состояние соответствий хранится в приватных метках Google-копий с хешами,
без исходных ID и содержания; название копии задаётся маршрутом. Повторный проход обнаруживает ранее сохранённые
копии после потери ответа или перезапуска. Управляются только копии своего
namespace; исходные и посторонние события не изменяются.

Все источники и назначения читаются со всеми страницами до записи. Ошибка
чтения отменяет весь план. После каждой записи результат читается обратно;
при ошибке проход останавливается, уже сохранённые изменения остаются, следующий
проход восстанавливает соответствия. Новая занятость создаётся до удаления
устаревших копий. Запускайте один экземпляр на одном хосте с одним конфигом:
блокировка файла предотвращает параллельные запуски этого конфига; распределённой
блокировки между VPS нет.

Защищается окно от текущего момента на `horizon_days` вперёд. Копии вне него
удаляются при успешном проходе; для продления защиты увеличьте горизонт.
Обычная задержка до двух минут ещё требует измерения на реальных аккаунтах.

## Проверки и сборка

```bash
make install       # зависимости из Cargo.lock
make check         # fmt-check, check, clippy, tests, release build, shell syntax
make build
make build-release
make build-local   # оптимизированный бинарник в ./gallendar без публикации
make test
make lint
make fmt
make version
make smoke-install # архив + SHA-256 + установка в изолированный каталог
```

Сборки и пакеты по умолчанию находятся в `~/construction_side/gallendar`;
`CARGO_TARGET_DIR` и `DIST_DIR` можно переопределить. После сборки release-бинарник
лежит в `~/construction_side/gallendar/target/release/gallendar`.

## Релизы

Публичный репозиторий: [RoTorEx/gallendar](https://github.com/RoTorEx/gallendar).
После коммита изменений и отдельного запроса
релиза выполнить интерактивно:

```bash
make release
# Версия MAJOR.MINOR.PATCH вводится в приглашении команды.
make release-push
```

Как у соседей: `make release` проверяет чистый `main`, получает теги, запускает
проверки, обновляет Cargo.toml/Cargo.lock и CHANGELOG, создаёт отдельный коммит
и annotated tag. Сам он не публикует. `make release-push` отправляет `main`
и annotated tags без force; release tag должен указывать на HEAD.

Текущий scope релизных сборок — только macOS Apple Silicon (M-чипы),
`aarch64-apple-darwin`. Intel Mac и Linux отложены; добавление платформ требует
отдельного согласования. Ранее опубликованные assets остаются доступными.
Push main и ручной запуск проверяют сборку без публикации. Tag запускает
проверку версии, упаковку, smoke install и GitHub Release с архивом
`gallendar-macos-aarch64.tar.gz`, SHA-256 и `gallendar-install.sh`.
Публичный installer по умолчанию использует `RoTorEx/gallendar` и принимает `--version`, `--archive`, `--install-dir`,
`--config-dir`, `--no-path-update`; репозиторий можно переопределить
через `GALLENDAR_INSTALL_REPO=OWNER/REPO`. Конфиги при обновлении сохраняются.

## Документы и Kernel

- [BUSINESS.md](BUSINESS.md) — назначение, поведение и границы продукта.
- [ROADMAP.md](ROADMAP.md) — состояние разработки и оставшиеся этапы.
- [CHANGELOG.md](CHANGELOG.md) — заметные изменения.
- [AGENTS.md](AGENTS.md) — маршрутизация рабочих инструкций.

```bash
make vibe-pull
# Если путь к Kernel изменился:
make vibe-kernel-set KERNEL=/absolute/path/to/vibecoding-kernel
make vibe-pull
```
