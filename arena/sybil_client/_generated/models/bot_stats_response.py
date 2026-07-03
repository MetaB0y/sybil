from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BotStatsResponse")



@_attrs_define
class BotStatsResponse:
    """ 
        Attributes:
            articles (int):
            decisions (int):
            snapshots (int):
            token_usage (int):
            traders (int):
            latest_decision_timestamp (None | str | Unset):
     """

    articles: int
    decisions: int
    snapshots: int
    token_usage: int
    traders: int
    latest_decision_timestamp: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        articles = self.articles

        decisions = self.decisions

        snapshots = self.snapshots

        token_usage = self.token_usage

        traders = self.traders

        latest_decision_timestamp: None | str | Unset
        if isinstance(self.latest_decision_timestamp, Unset):
            latest_decision_timestamp = UNSET
        else:
            latest_decision_timestamp = self.latest_decision_timestamp


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "articles": articles,
            "decisions": decisions,
            "snapshots": snapshots,
            "token_usage": token_usage,
            "traders": traders,
        })
        if latest_decision_timestamp is not UNSET:
            field_dict["latest_decision_timestamp"] = latest_decision_timestamp

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        articles = d.pop("articles")

        decisions = d.pop("decisions")

        snapshots = d.pop("snapshots")

        token_usage = d.pop("token_usage")

        traders = d.pop("traders")

        def _parse_latest_decision_timestamp(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        latest_decision_timestamp = _parse_latest_decision_timestamp(d.pop("latest_decision_timestamp", UNSET))


        bot_stats_response = cls(
            articles=articles,
            decisions=decisions,
            snapshots=snapshots,
            token_usage=token_usage,
            traders=traders,
            latest_decision_timestamp=latest_decision_timestamp,
        )


        bot_stats_response.additional_properties = d
        return bot_stats_response

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
