"""Quick validate that the architecture SVGs parse as well-formed XML."""
import xml.etree.ElementTree as ET
import sys

paths = [
    "pitch-deck/arsitektur-sistem-dark.svg",
    "pitch-deck/arsitektur-sistem-light.svg",
]
for p in paths:
    try:
        tree = ET.parse(p)
        root = tree.getroot()
        print(f"OK: {p}  (root tag = {root.tag})")
    except Exception as e:
        print(f"FAIL: {p}  -> {e}")
        sys.exit(1)
print("All SVGs valid.")
