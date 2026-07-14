from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.actor_market_receipt import ActorMarketReceipt





T = TypeVar("T", bound="ActorEpochResponse")



@_attrs_define
class ActorEpochResponse:
    """ 
        Attributes:
            accepted (bool):
            accepted_orders (int):
            considered (int):
            markets (list[ActorMarketReceipt]):
            principal_id (str):
            selected (int):
            skipped (int):
            target_height (int):
            universe_generation (int):
     """

    accepted: bool
    accepted_orders: int
    considered: int
    markets: list[ActorMarketReceipt]
    principal_id: str
    selected: int
    skipped: int
    target_height: int
    universe_generation: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.actor_market_receipt import ActorMarketReceipt
        accepted = self.accepted

        accepted_orders = self.accepted_orders

        considered = self.considered

        markets = []
        for markets_item_data in self.markets:
            markets_item = markets_item_data.to_dict()
            markets.append(markets_item)



        principal_id = self.principal_id

        selected = self.selected

        skipped = self.skipped

        target_height = self.target_height

        universe_generation = self.universe_generation


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "accepted": accepted,
            "accepted_orders": accepted_orders,
            "considered": considered,
            "markets": markets,
            "principal_id": principal_id,
            "selected": selected,
            "skipped": skipped,
            "target_height": target_height,
            "universe_generation": universe_generation,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.actor_market_receipt import ActorMarketReceipt
        d = dict(src_dict)
        accepted = d.pop("accepted")

        accepted_orders = d.pop("accepted_orders")

        considered = d.pop("considered")

        markets = []
        _markets = d.pop("markets")
        for markets_item_data in (_markets):
            markets_item = ActorMarketReceipt.from_dict(markets_item_data)



            markets.append(markets_item)


        principal_id = d.pop("principal_id")

        selected = d.pop("selected")

        skipped = d.pop("skipped")

        target_height = d.pop("target_height")

        universe_generation = d.pop("universe_generation")

        actor_epoch_response = cls(
            accepted=accepted,
            accepted_orders=accepted_orders,
            considered=considered,
            markets=markets,
            principal_id=principal_id,
            selected=selected,
            skipped=skipped,
            target_height=target_height,
            universe_generation=universe_generation,
        )


        actor_epoch_response.additional_properties = d
        return actor_epoch_response

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
