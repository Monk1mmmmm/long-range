"""Minimal parser for Supreme Commander .bp blueprint files (Lua table literals)."""
import re

TOKEN = re.compile(r"""
    (?P<ws>\s+|\#[^\n]*|--[^\n]*)
  | (?P<str>'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*")
  | (?P<num>-?(?:0x[0-9a-fA-F]+|\d+\.?\d*(?:[eE][-+]?\d+)?|\.\d+))
  | (?P<id>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<sym>[{}=,;()\[\]])
""", re.X)


def tokenize(text):
    pos, out = 0, []
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            pos += 1
            continue
        pos = m.end()
        if m.lastgroup != "ws":
            out.append((m.lastgroup, m.group()))
    return out


class P:
    def __init__(self, toks):
        self.t, self.i = toks, 0

    def peek(self, k=0):
        return self.t[self.i + k] if self.i + k < len(self.t) else ("eof", "")

    def next(self):
        tok = self.peek()
        self.i += 1
        return tok

    def value(self):
        kind, v = self.next()
        if kind == "str":
            return v[1:-1]
        if kind == "num":
            return float(v) if not v.startswith("0x") else int(v, 16)
        if kind == "id":
            if v == "true":
                return True
            if v == "false":
                return False
            if v == "nil":
                return None
            if self.peek()[1] == "{":      # Sound { ... } style constructor
                return self.value()
            if self.peek()[1] == "(":
                self.next()
                val = self.value()
                self.next()
                return val
            return v
        if v == "{":
            return self.table()
        return None

    def table(self):
        d, lst = {}, []
        while True:
            kind, v = self.peek()
            if v == "}" or kind == "eof":
                self.next()
                break
            if v in ",;":
                self.next()
                continue
            if kind == "id" and self.peek(1)[1] == "=":
                self.next(); self.next()
                d[v] = self.value()
            elif v == "[":
                self.next(); k = self.value(); self.next(); self.next()
                d[k] = self.value()
            else:
                lst.append(self.value())
        if lst and not d:
            return lst
        if lst:
            d["_list"] = lst
        return d


def load(path):
    text = open(path, encoding="latin1").read()
    p = P(tokenize(text))
    # Skip to the first '{' (after UnitBlueprint / ProjectileBlueprint).
    while p.peek()[1] != "{" and p.peek()[0] != "eof":
        p.next()
    return p.value()
