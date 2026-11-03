# Montážny návod

Plugin pre Open CAD Studio na tvorbu montážnych postupov zo zostáv DWG/DXF.

## Verzia 0.3.0

### Montážne kroky
- označ diely v DWG,
- **Priradiť diely** ich uloží do aktuálneho montážneho kroku,
- číslo kroku sa zapisuje priamo do DWG/DXF ako XDATA aplikácie `MONTAZ_NAVOD`,
- **Predch. krok / Ďalší krok** prepína aktuálny krok,
- **Zobraziť krok** zobrazí kumulatívnu zostavu od kroku 1 po aktuálny krok,
- **Iba nové diely** izoluje len diely pridané v aktuálnom kroku,
- **Zobraziť všetko** ukončí izoláciu.

### Affinity
Tlačidlo **Export SVG** spustí natívny Open CAD Studio `EXPORTSVG`.

Odporúčaný pracovný priečinok:

```text
PROJEKT/
  skrinka.dwg
  AFFINITY/
    KROK_01.svg
    KROK_02.svg
    KROK_03.svg
```

SVG vlož v Affinity ako **Linked resource**. Pri ďalšom exporte prepíš rovnaký
`KROK_XX.svg`; Affinity potom vie načítať aktualizovaný externý zdroj.

## Inštalácia

Plugin Manager → Add from GitHub:

```text
https://github.com/martindrabdrab-sketch/montazny-navod.git
```

Plugin cieli na Open CAD Studio v2026.44 / Plugin API 14.
