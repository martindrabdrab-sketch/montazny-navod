# Montážny návod

Samostatný plugin pre Open CAD Studio.

## Funkcie 0.1.0

- **Izolovať** — ponechá viditeľné iba označené objekty.
- **Skryť** — skryje označené objekty.
- **Zobraziť všetko** — obnoví objekty skryté cez Hide/Isolate.
- **Výber** — vypíše počet označených objektov.

Plugin používa natívne príkazy Open CAD Studio:
`ISOLATEOBJECTS`, `HIDEOBJECTS` a `UNISOLATEOBJECTS`.

## Git repository pre Plugin Manager

```text
https://github.com/martindrabdrab-sketch/montazny-navod.git
```

## Lokálny build

```bash
cargo build --release
```

Plugin je zostavovaný ako samostatná `cdylib` podľa oficiálneho Open CAD Studio plugin template.
