from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.account_fill_response import AccountFillResponse





T = TypeVar("T", bound="AccountFillPageResponse")



@_attrs_define
class AccountFillPageResponse:
    """ 
        Attributes:
            cursor_gap (bool): The supplied forward cursor may have skipped pruned fills.
            fills (list[AccountFillResponse]):
            history_scope (str):
            history_truncated (bool): True means rows older than the retention boundary are unavailable.
            history_complete_from_height (int | None | Unset):
            indexed_through_height (int | None | Unset):
            next_after (None | str | Unset): Cursor to continue forward pagination, when this was a forward page.
            pruned_through_height (int | None | Unset): Highest block from which this account had a fill row removed.
            retention_min_timestamp_ms (int | None | Unset):
     """

    cursor_gap: bool
    fills: list[AccountFillResponse]
    history_scope: str
    history_truncated: bool
    history_complete_from_height: int | None | Unset = UNSET
    indexed_through_height: int | None | Unset = UNSET
    next_after: None | str | Unset = UNSET
    pruned_through_height: int | None | Unset = UNSET
    retention_min_timestamp_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.account_fill_response import AccountFillResponse
        cursor_gap = self.cursor_gap

        fills = []
        for fills_item_data in self.fills:
            fills_item = fills_item_data.to_dict()
            fills.append(fills_item)



        history_scope = self.history_scope

        history_truncated = self.history_truncated

        history_complete_from_height: int | None | Unset
        if isinstance(self.history_complete_from_height, Unset):
            history_complete_from_height = UNSET
        else:
            history_complete_from_height = self.history_complete_from_height

        indexed_through_height: int | None | Unset
        if isinstance(self.indexed_through_height, Unset):
            indexed_through_height = UNSET
        else:
            indexed_through_height = self.indexed_through_height

        next_after: None | str | Unset
        if isinstance(self.next_after, Unset):
            next_after = UNSET
        else:
            next_after = self.next_after

        pruned_through_height: int | None | Unset
        if isinstance(self.pruned_through_height, Unset):
            pruned_through_height = UNSET
        else:
            pruned_through_height = self.pruned_through_height

        retention_min_timestamp_ms: int | None | Unset
        if isinstance(self.retention_min_timestamp_ms, Unset):
            retention_min_timestamp_ms = UNSET
        else:
            retention_min_timestamp_ms = self.retention_min_timestamp_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "cursor_gap": cursor_gap,
            "fills": fills,
            "history_scope": history_scope,
            "history_truncated": history_truncated,
        })
        if history_complete_from_height is not UNSET:
            field_dict["history_complete_from_height"] = history_complete_from_height
        if indexed_through_height is not UNSET:
            field_dict["indexed_through_height"] = indexed_through_height
        if next_after is not UNSET:
            field_dict["next_after"] = next_after
        if pruned_through_height is not UNSET:
            field_dict["pruned_through_height"] = pruned_through_height
        if retention_min_timestamp_ms is not UNSET:
            field_dict["retention_min_timestamp_ms"] = retention_min_timestamp_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.account_fill_response import AccountFillResponse
        d = dict(src_dict)
        cursor_gap = d.pop("cursor_gap")

        fills = []
        _fills = d.pop("fills")
        for fills_item_data in (_fills):
            fills_item = AccountFillResponse.from_dict(fills_item_data)



            fills.append(fills_item)


        history_scope = d.pop("history_scope")

        history_truncated = d.pop("history_truncated")

        def _parse_history_complete_from_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        history_complete_from_height = _parse_history_complete_from_height(d.pop("history_complete_from_height", UNSET))


        def _parse_indexed_through_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        indexed_through_height = _parse_indexed_through_height(d.pop("indexed_through_height", UNSET))


        def _parse_next_after(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        next_after = _parse_next_after(d.pop("next_after", UNSET))


        def _parse_pruned_through_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pruned_through_height = _parse_pruned_through_height(d.pop("pruned_through_height", UNSET))


        def _parse_retention_min_timestamp_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        retention_min_timestamp_ms = _parse_retention_min_timestamp_ms(d.pop("retention_min_timestamp_ms", UNSET))


        account_fill_page_response = cls(
            cursor_gap=cursor_gap,
            fills=fills,
            history_scope=history_scope,
            history_truncated=history_truncated,
            history_complete_from_height=history_complete_from_height,
            indexed_through_height=indexed_through_height,
            next_after=next_after,
            pruned_through_height=pruned_through_height,
            retention_min_timestamp_ms=retention_min_timestamp_ms,
        )


        account_fill_page_response.additional_properties = d
        return account_fill_page_response

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
