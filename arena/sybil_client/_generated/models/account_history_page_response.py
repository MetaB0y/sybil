from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.history_event_response import HistoryEventResponse





T = TypeVar("T", bound="AccountHistoryPageResponse")



@_attrs_define
class AccountHistoryPageResponse:
    """ 
        Attributes:
            events (list[HistoryEventResponse]):
            history_scope (str):
            history_truncated (bool):
            next_before (None | str | Unset):
            retention_min_timestamp_ms (int | None | Unset):
     """

    events: list[HistoryEventResponse]
    history_scope: str
    history_truncated: bool
    next_before: None | str | Unset = UNSET
    retention_min_timestamp_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.history_event_response import HistoryEventResponse
        events = []
        for events_item_data in self.events:
            events_item = events_item_data.to_dict()
            events.append(events_item)



        history_scope = self.history_scope

        history_truncated = self.history_truncated

        next_before: None | str | Unset
        if isinstance(self.next_before, Unset):
            next_before = UNSET
        else:
            next_before = self.next_before

        retention_min_timestamp_ms: int | None | Unset
        if isinstance(self.retention_min_timestamp_ms, Unset):
            retention_min_timestamp_ms = UNSET
        else:
            retention_min_timestamp_ms = self.retention_min_timestamp_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "events": events,
            "history_scope": history_scope,
            "history_truncated": history_truncated,
        })
        if next_before is not UNSET:
            field_dict["next_before"] = next_before
        if retention_min_timestamp_ms is not UNSET:
            field_dict["retention_min_timestamp_ms"] = retention_min_timestamp_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.history_event_response import HistoryEventResponse
        d = dict(src_dict)
        events = []
        _events = d.pop("events")
        for events_item_data in (_events):
            events_item = HistoryEventResponse.from_dict(events_item_data)



            events.append(events_item)


        history_scope = d.pop("history_scope")

        history_truncated = d.pop("history_truncated")

        def _parse_next_before(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        next_before = _parse_next_before(d.pop("next_before", UNSET))


        def _parse_retention_min_timestamp_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        retention_min_timestamp_ms = _parse_retention_min_timestamp_ms(d.pop("retention_min_timestamp_ms", UNSET))


        account_history_page_response = cls(
            events=events,
            history_scope=history_scope,
            history_truncated=history_truncated,
            next_before=next_before,
            retention_min_timestamp_ms=retention_min_timestamp_ms,
        )


        account_history_page_response.additional_properties = d
        return account_history_page_response

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
