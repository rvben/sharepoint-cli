"""Build sanitized response-shape fixtures from pinned public PnP recordings.

Raw responses stay in memory. This imports existing third-party evidence; it
does not capture a tenant or establish that the CLI's requests work online.
"""

import hashlib
import json
import re
import urllib.parse
import urllib.request
from pathlib import Path

COMMIT = "aba47bf58400a26ac7220997017be36d54d501fe"
BASE = f"https://raw.githubusercontent.com/pnp/pnpcore/{COMMIT}/"
PREFIX = "src/sdk/PnP.Core.Test/SharePoint/MockData/"
SOURCES = {
    "identity": (
        "WebTests/GetWebCurrentUserTest-0-00002.response.json",
        "156d13a5e27e5ed6819ce20967b0aa6922b52b6c5c26b46d43bf4e00b07fc81d",
    ),
    "file": (
        "FilesTests/GetFileByServerRelativeUrlTest-0-00004.response.json",
        "9567b66853e74180eb96c070bdb0b730c8480f53abc2325ab9b9b8cf84d628ac",
    ),
    "folder": (
        "FoldersTests/GetFolderByServerRelativeUrlTest-0-00002.response.json",
        "b5763e529ae3fd2374bcea4c1fe72745e485791b01eda8de5bdad83e28f4139d",
    ),
    "missing": (
        "FoldersTests/GetFolderById_NonExistentFolder_ShouldThrowCorrectException-0-00002.response.json",
        "f2754f69d20600acfa918bb3810d6263b04952b2c9164567db9e50f3c08c53dd",
    ),
    "search_first": (
        "WebTests/SearchSortingPagingTest-0-00002.response.json",
        "21c09efbfffa1a537acb2e3224683c3c5a802a6b9ba4f2da1ae039ce13ae4d42",
    ),
    "search_second": (
        "WebTests/SearchSortingPagingTest-0-00003.response.json",
        "6fe21462defcfa742b16862883a1ed5e469320a78f7b1798c5ed9b48909d4836",
    ),
}
DESTINATION = Path(__file__).resolve().parents[1] / "examples/simulator/contracts"
HOST = "simulator.sharepoint.com"
SITE = "/sites/Marketing"
ROOT = f"{SITE}/Shared Documents"
API = f"{SITE}/_api/web"
FILE_FIELDS = "UniqueId,Name,ServerRelativeUrl,Length,ETag,TimeCreated,TimeLastModified"
FOLDER_FIELDS = "UniqueId,Name,ServerRelativeUrl,TimeCreated,TimeLastModified"


def download(path):
    request = urllib.request.Request(
        BASE + path, headers={"User-Agent": "sharepoint-cli-contract-import"}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def project(body, fields):
    return {field: body[field] for field in fields.split(",")}


def identifier(index):
    return f"00000000-0000-4000-8000-{index:012x}"


def search_body(body, start):
    relevant = body["PrimaryQueryResult"]["RelevantResults"]
    result = project(relevant, "RowCount,TotalRows,TotalRowsIncludingDuplicates")
    rows = []
    for index, row in enumerate(relevant["Table"]["Rows"], start + 1):
        url = f"https://{HOST}{ROOT}/result-{index:02}.txt"
        replacements = {
            "Path": url,
            "Url": url,
            "OriginalPath": url,
            "Title": f"Result {index:02}",
            "UniqueId": identifier(index + 100),
            "ListId": identifier(1),
            "SiteId": identifier(2),
            "WebId": identifier(2),
            "ResultTypeIdList": "0",
            "ResultTypeId": "0",
            "RenderTemplateId": "~sitecollection/_catalogs/masterpage/Display Templates/Search/Item_Default.js",
            "piSearchResultId": f"0_{index}_1_1",
            "DocId": str(index),
            "Rank": "1.0",
            "GeoLocationSource": "EMEA",
            "contentclass": "STS_ListItem_DocumentLibrary",
            "IsExternalContent": "false",
            "UrlZone": "0",
            "ModifiedBy": "Recorded User",
        }
        cells = []
        for cell in row["Cells"]:
            value = cell["Value"]
            # An unfamiliar field requires an explicit sanitization decision.
            replacement = None if value is None else replacements[cell["Key"]]
            if type(value) is not type(replacement):
                raise ValueError("sanitization must preserve recorded cell value types")
            cells.append(
                {
                    "Key": cell["Key"],
                    "Value": replacement,
                    "ValueType": cell["ValueType"],
                }
            )
        rows.append({"Cells": cells})
    result["Table"] = {"Rows": rows}
    return {"PrimaryQueryResult": {"RelevantResults": result}}


def build(recorded):
    bodies = {
        name: json.loads(response["Response"]) for name, response in recorded.items()
    }
    exchanges = []

    def add(path, body, query=None, status=200):
        exchanges.append(
            {"path": path, "query": query or {}, "status": status, "body": body}
        )

    def resource(kind, path):
        return urllib.parse.quote(
            f"{API}/Get{kind}ByServerRelativePath(decodedurl='{path}')", safe="/()='"
        )

    # These two routing/setup responses are synthetic, not captured envelopes.
    add(
        API,
        {"Id": identifier(2), "Title": "Marketing", "Url": f"https://{HOST}{SITE}"},
        {"$select": "Id,Title,Url"},
    )
    add(
        API + "/lists",
        {
            "value": [
                {
                    "Id": identifier(1),
                    "Title": "Documents",
                    "BaseTemplate": 101,
                    "RootFolder": {"ServerRelativeUrl": ROOT},
                }
            ]
        },
        {
            "$filter": "(BaseTemplate eq 101 or BaseTemplate eq 700) and Hidden eq false",
            "$select": "Id,Title,BaseTemplate,RootFolder/ServerRelativeUrl",
            "$expand": "RootFolder",
        },
    )

    identity = project(bodies["identity"], "Title,Email,LoginName")
    identity.update(
        Title="Recorded User",
        Email="recorded@example.test",
        LoginName="i:0#.f|membership|recorded@example.test",
    )
    add("/_api/web/currentuser", identity, {"$select": "Title,Email,LoginName"})

    file = project(bodies["file"], FILE_FIELDS)
    file.update(
        UniqueId=identifier(3), Name="hello.txt", ServerRelativeUrl=ROOT + "/hello.txt"
    )
    file["ETag"] = re.sub(
        r"\{[0-9A-Fa-f-]+\}", "{" + identifier(3).upper() + "}", file["ETag"]
    )
    add(resource("File", ROOT + "/hello.txt"), file, {"$select": FILE_FIELDS})

    folder = project(bodies["folder"], FOLDER_FIELDS)
    folder.update(
        UniqueId=identifier(4), Name="Plans", ServerRelativeUrl=ROOT + "/Plans"
    )
    missing = bodies["missing"]
    if missing["odata.error"]["message"]["value"] != "File Not Found.":
        raise ValueError("review changed upstream error text before importing")
    for name in ["Plans", "missing.txt"]:
        add(resource("File", ROOT + "/" + name), missing, {"$select": FILE_FIELDS}, 404)
    add(resource("Folder", ROOT + "/Plans"), folder, {"$select": FOLDER_FIELDS})
    add(
        resource("Folder", ROOT + "/missing.txt"),
        missing,
        {"$select": FOLDER_FIELDS},
        404,
    )

    for name, start in [("search_first", 0), ("search_second", 10)]:
        add(
            "/_api/search/query",
            search_body(bodies[name], start),
            {
                "querytext": f"'(*) path:\"https://{HOST}{ROOT}\" IsDocument:1'",
                "selectproperties": "'Title,Path,FileName,Size,LastModifiedTime,UniqueId'",
                "rowlimit": "50",
                "startrow": str(start),
            },
        )
    return {
        "recording": {
            "host": HOST,
            "source": f"Sanitized/projected PnP Core SDK server-response recordings at {COMMIT}; synthetic routing and setup. See contracts/README.md and provenance.json. Not a live CLI capture.",
            "exchanges": exchanges,
        }
    }


def main():
    recorded = {}
    provenance = {
        "repository": "https://github.com/pnp/pnpcore",
        "commit": COMMIT,
        "sources": {},
    }
    for name, (path, expected_hash) in SOURCES.items():
        raw = download(PREFIX + path)
        digest = hashlib.sha256(raw).hexdigest()
        if digest != expected_hash:
            raise ValueError(f"upstream SHA-256 mismatch for {name}")
        response = json.loads(raw)
        if response["StatusCode"] != (404 if name == "missing" else 200):
            raise ValueError(f"unexpected upstream HTTP status for {name}")
        recorded[name] = response
        provenance["sources"][name] = {
            "url": BASE + PREFIX + path,
            "sha256": digest,
            "status": response["StatusCode"],
        }
    fixture = build(recorded)
    license_text = download("LICENSE").decode("utf-8")
    if not license_text.startswith("MIT License\n\nCopyright (c) 2020 PnP"):
        raise ValueError("review upstream license before importing")
    DESTINATION.mkdir(parents=True, exist_ok=True)
    for name, value in [("pnpcore.json", fixture), ("provenance.json", provenance)]:
        (DESTINATION / name).write_text(
            json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
    (DESTINATION / "LICENSE.pnpcore").write_text(license_text, encoding="utf-8")
    print(
        f"Imported {len(recorded)} pinned recordings into {len(fixture['recording']['exchanges'])} adapted exchanges."
    )


if __name__ == "__main__":
    main()
