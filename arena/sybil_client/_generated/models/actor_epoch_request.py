from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.actor_market_intent import ActorMarketIntent





T = TypeVar("T", bound="ActorEpochRequest")



@_attrs_define
class ActorEpochRequest:
    """ 
        Attributes:
            epoch_id (str):
            market_intents (list[ActorMarketIntent]):
            observed_at_ms (int):
            target_height (int):
            universe_generation (int):
            valid_until_ms (int):
            mm_budget_nanos (int | None | Unset): Shared market-maker capital limit. Integer nanodollars;
                1_000_000_000 = $1. Forbidden for noise actors.
     """

    epoch_id: str
    market_intents: list[ActorMarketIntent]
    observed_at_ms: int
    target_height: int
    universe_generation: int
    valid_until_ms: int
    mm_budget_nanos: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.actor_market_intent import ActorMarketIntent
        epoch_id = self.epoch_id

        market_intents = []
        for market_intents_item_data in self.market_intents:
            market_intents_item = market_intents_item_data.to_dict()
            market_intents.append(market_intents_item)



        observed_at_ms = self.observed_at_ms

        target_height = self.target_height

        universe_generation = self.universe_generation

        valid_until_ms = self.valid_until_ms

        mm_budget_nanos: int | None | Unset
        if isinstance(self.mm_budget_nanos, Unset):
            mm_budget_nanos = UNSET
        else:
            mm_budget_nanos = self.mm_budget_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "epoch_id": epoch_id,
            "market_intents": market_intents,
            "observed_at_ms": observed_at_ms,
            "target_height": target_height,
            "universe_generation": universe_generation,
            "valid_until_ms": valid_until_ms,
        })
        if mm_budget_nanos is not UNSET:
            field_dict["mm_budget_nanos"] = mm_budget_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.actor_market_intent import ActorMarketIntent
        d = dict(src_dict)
        epoch_id = d.pop("epoch_id")

        market_intents = []
        _market_intents = d.pop("market_intents")
        for market_intents_item_data in (_market_intents):
            market_intents_item = ActorMarketIntent.from_dict(market_intents_item_data)



            market_intents.append(market_intents_item)


        observed_at_ms = d.pop("observed_at_ms")

        target_height = d.pop("target_height")

        universe_generation = d.pop("universe_generation")

        valid_until_ms = d.pop("valid_until_ms")

        def _parse_mm_budget_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        mm_budget_nanos = _parse_mm_budget_nanos(d.pop("mm_budget_nanos", UNSET))


        actor_epoch_request = cls(
            epoch_id=epoch_id,
            market_intents=market_intents,
            observed_at_ms=observed_at_ms,
            target_height=target_height,
            universe_generation=universe_generation,
            valid_until_ms=valid_until_ms,
            mm_budget_nanos=mm_budget_nanos,
        )


        actor_epoch_request.additional_properties = d
        return actor_epoch_request

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
