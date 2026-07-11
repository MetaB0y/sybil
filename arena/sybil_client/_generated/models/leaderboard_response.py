from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.leaderboard_entry_response import LeaderboardEntryResponse





T = TypeVar("T", bound="LeaderboardResponse")



@_attrs_define
class LeaderboardResponse:
    """ 
        Attributes:
            entries (list[LeaderboardEntryResponse]): Ranked entries, best PnL first. Ties break by ascending account id.
            window (str): Window this leaderboard was ranked over: `7d`, `30d`, or `all`.
     """

    entries: list[LeaderboardEntryResponse]
    window: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.leaderboard_entry_response import LeaderboardEntryResponse
        entries = []
        for entries_item_data in self.entries:
            entries_item = entries_item_data.to_dict()
            entries.append(entries_item)



        window = self.window


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "entries": entries,
            "window": window,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.leaderboard_entry_response import LeaderboardEntryResponse
        d = dict(src_dict)
        entries = []
        _entries = d.pop("entries")
        for entries_item_data in (_entries):
            entries_item = LeaderboardEntryResponse.from_dict(entries_item_data)



            entries.append(entries_item)


        window = d.pop("window")

        leaderboard_response = cls(
            entries=entries,
            window=window,
        )


        leaderboard_response.additional_properties = d
        return leaderboard_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
