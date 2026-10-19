"""題庫管線共用路徑設定。皆可用環境變數覆寫，預設值沿用原本的本機路徑。

- JAMIE_EXAM_ROOT : 題庫素材根目錄（內含 data/scans/<科目>/*.jpg）
- TUTOR_DATA_DIR  : 後端 tutor_data 目錄（產出寫到這裡，後端會自動匯入 questions*.json）
"""
import os

_REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

JAMIE_EXAM_ROOT = os.environ.get("JAMIE_EXAM_ROOT", r"C:\workspace\jamie-exam")
SCANS_DIR = os.path.join(JAMIE_EXAM_ROOT, "data", "scans")
DATA_DIR = os.environ.get("TUTOR_DATA_DIR", os.path.join(_REPO_ROOT, "backend", "tutor_data"))
SCRATCH_DIR = os.path.join(_REPO_ROOT, "scratch")
