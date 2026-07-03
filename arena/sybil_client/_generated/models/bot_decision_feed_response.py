from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.bot_decision_response import BotDecisionResponse
  from ..models.bot_stats_response import BotStatsResponse
  from ..models.bot_summary_response import BotSummaryResponse
  from ..models.token_usage_response import TokenUsageResponse





T = TypeVar("T", bound="BotDecisionFeedResponse")



@_attrs_define
class BotDecisionFeedResponse:
    """ 
        Attributes:
            db_available (bool):
            decisions (list[BotDecisionResponse]):
            stats (BotStatsResponse):
            summaries (list[BotSummaryResponse]):
            token_usage (list[TokenUsageResponse]):
            db_path (None | str | Unset):
            error (None | str | Unset):
     """

    db_available: bool
    decisions: list[BotDecisionResponse]
    stats: BotStatsResponse
    summaries: list[BotSummaryResponse]
    token_usage: list[TokenUsageResponse]
    db_path: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.bot_decision_response import BotDecisionResponse
        from ..models.bot_stats_response import BotStatsResponse
        from ..models.bot_summary_response import BotSummaryResponse
        from ..models.token_usage_response import TokenUsageResponse
        db_available = self.db_available

        decisions = []
        for decisions_item_data in self.decisions:
            decisions_item = decisions_item_data.to_dict()
            decisions.append(decisions_item)



        stats = self.stats.to_dict()

        summaries = []
        for summaries_item_data in self.summaries:
            summaries_item = summaries_item_data.to_dict()
            summaries.append(summaries_item)



        token_usage = []
        for token_usage_item_data in self.token_usage:
            token_usage_item = token_usage_item_data.to_dict()
            token_usage.append(token_usage_item)



        db_path: None | str | Unset
        if isinstance(self.db_path, Unset):
            db_path = UNSET
        else:
            db_path = self.db_path

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "db_available": db_available,
            "decisions": decisions,
            "stats": stats,
            "summaries": summaries,
            "token_usage": token_usage,
        })
        if db_path is not UNSET:
            field_dict["db_path"] = db_path
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bot_decision_response import BotDecisionResponse
        from ..models.bot_stats_response import BotStatsResponse
        from ..models.bot_summary_response import BotSummaryResponse
        from ..models.token_usage_response import TokenUsageResponse
        d = dict(src_dict)
        db_available = d.pop("db_available")

        decisions = []
        _decisions = d.pop("decisions")
        for decisions_item_data in (_decisions):
            decisions_item = BotDecisionResponse.from_dict(decisions_item_data)



            decisions.append(decisions_item)


        stats = BotStatsResponse.from_dict(d.pop("stats"))




        summaries = []
        _summaries = d.pop("summaries")
        for summaries_item_data in (_summaries):
            summaries_item = BotSummaryResponse.from_dict(summaries_item_data)



            summaries.append(summaries_item)


        token_usage = []
        _token_usage = d.pop("token_usage")
        for token_usage_item_data in (_token_usage):
            token_usage_item = TokenUsageResponse.from_dict(token_usage_item_data)



            token_usage.append(token_usage_item)


        def _parse_db_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        db_path = _parse_db_path(d.pop("db_path", UNSET))


        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))


        bot_decision_feed_response = cls(
            db_available=db_available,
            decisions=decisions,
            stats=stats,
            summaries=summaries,
            token_usage=token_usage,
            db_path=db_path,
            error=error,
        )


        bot_decision_feed_response.additional_properties = d
        return bot_decision_feed_response

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
