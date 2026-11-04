from __future__ import annotations

import json
import re
from pathlib import Path

_NAME = r'[^\s，,、；;。:：/kV“”]+?'
_PARENS = re.compile(r'（[^（）]*）|\([^()]*\)')
_STATION = re.compile(r'(?<![\d.])\d+kV' + _NAME + r'站')
_LINE = re.compile(r'(?<![\d.])(\d+(?:\.\d+)?kV)(' + _NAME + r'线)')
_POLE = re.compile(r'(\d+(?:\+\d+)?)号(?:杆|塔)|#(\d+(?:\+\d+)?)(?:杆)?')
_NUMBER = re.compile(r'\d+(?=接地刀闸|开关|刀闸|令克)')


def load_lexicon(path: str | Path) -> dict:
    lexicon = json.loads(Path(path).read_text(encoding="utf-8"))
    for field in ("synonyms", "keywords"):
        if field not in lexicon:
            raise ValueError(f"{path}：缺少字段 {field}")
    return lexicon


def normalize(text: str, lexicon: dict) -> str:
    text = re.sub(r'\s+', '', text).replace('！', '!').replace('，', ',')
    variants = [(variant, canonical) for canonical, others in lexicon["synonyms"].items()
                for variant in others]
    for variant, canonical in sorted(variants, key=lambda pair: len(pair[0]), reverse=True):
        text = text.replace(variant, canonical)
    return text


def _is_line(name: str) -> bool:
    return not name.endswith(("进线", "出线", "引线", "引流线", "接线", "母线", "导线", "地线", "跳线")) and not any(
        word in name for word in ("开闭所", "公变", "专变", "开关", "刀闸", "站"))


def lines(text: str) -> set[str]:
    return {voltage + re.sub(r'(?<=\D)\d{3}(?=线)', '', name)
            for voltage, name in _LINE.findall(_PARENS.sub('', text)) if _is_line(name)}


def stations(text: str) -> set[str]:
    return set(_STATION.findall(_PARENS.sub('', text)))


def poles(text: str) -> list[str]:
    return [match[1] or match[2] for match in _POLE.finditer(text)]


def abstract(text: str) -> str:
    text = _STATION.sub('{站}', text)
    text = _LINE.sub(lambda match: '{线路}' if _is_line(match[2]) else match[0], text)
    text = _POLE.sub('{杆号}', text)
    return _NUMBER.sub('{编号}', text)


def keywords(text: str, lexicon: dict) -> list[str]:
    text = normalize(text, lexicon)
    return [word for word in lexicon["keywords"] if normalize(word, lexicon) in text]
