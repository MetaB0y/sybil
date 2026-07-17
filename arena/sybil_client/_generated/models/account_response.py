from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.position_response import PositionResponse





T = TypeVar("T", bound="AccountResponse")



@_attrs_define
class AccountResponse:
    """ 
        Attributes:
            account_id (int):
            available_balance_nanos (str): Spendable account balance after live-order reservations. Integer
                nanodollars; 1_000_000_000 = $1.
            balance_nanos (str): Total (gross) account balance; see `available_balance_nanos` for spendable
                funds. Integer nanodollars; 1_000_000_000 = $1.
            events_digest_hex (str): Current event-chain digest used to make every key operation one-shot.
            keys_digest_hex (str): Current validity key-set digest used to state-bind key operations.
            reserved_balance_nanos (str): Balance reserved by live resting orders. Integer nanodollars;
                1_000_000_000 = $1.
            avatar_seed (None | str | Unset): Optional deterministic identicon seed (SYB-60).
            display_name (None | str | Unset): Optional public display name. A non-empty value opts this account into
                publication of its leaderboard financial row.
            positions (list[PositionResponse] | Unset):
     """

    account_id: int
    available_balance_nanos: str
    balance_nanos: str
    events_digest_hex: str
    keys_digest_hex: str
    reserved_balance_nanos: str
    avatar_seed: None | str | Unset = UNSET
    display_name: None | str | Unset = UNSET
    positions: list[PositionResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.position_response import PositionResponse
        account_id = self.account_id

        available_balance_nanos = self.available_balance_nanos

        balance_nanos = self.balance_nanos

        events_digest_hex = self.events_digest_hex

        keys_digest_hex = self.keys_digest_hex

        reserved_balance_nanos = self.reserved_balance_nanos

        avatar_seed: None | str | Unset
        if isinstance(self.avatar_seed, Unset):
            avatar_seed = UNSET
        else:
            avatar_seed = self.avatar_seed

        display_name: None | str | Unset
        if isinstance(self.display_name, Unset):
            display_name = UNSET
        else:
            display_name = self.display_name

        positions: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.positions, Unset):
            positions = []
            for positions_item_data in self.positions:
                positions_item = positions_item_data.to_dict()
                positions.append(positions_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "available_balance_nanos": available_balance_nanos,
            "balance_nanos": balance_nanos,
            "events_digest_hex": events_digest_hex,
            "keys_digest_hex": keys_digest_hex,
            "reserved_balance_nanos": reserved_balance_nanos,
        })
        if avatar_seed is not UNSET:
            field_dict["avatar_seed"] = avatar_seed
        if display_name is not UNSET:
            field_dict["display_name"] = display_name
        if positions is not UNSET:
            field_dict["positions"] = positions

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.position_response import PositionResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        available_balance_nanos = d.pop("available_balance_nanos")

        balance_nanos = d.pop("balance_nanos")

        events_digest_hex = d.pop("events_digest_hex")

        keys_digest_hex = d.pop("keys_digest_hex")

        reserved_balance_nanos = d.pop("reserved_balance_nanos")

        def _parse_avatar_seed(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        avatar_seed = _parse_avatar_seed(d.pop("avatar_seed", UNSET))


        def _parse_display_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        display_name = _parse_display_name(d.pop("display_name", UNSET))


        _positions = d.pop("positions", UNSET)
        positions: list[PositionResponse] | Unset = UNSET
        if _positions is not UNSET:
            positions = []
            for positions_item_data in _positions:
                positions_item = PositionResponse.from_dict(positions_item_data)



                positions.append(positions_item)


        account_response = cls(
            account_id=account_id,
            available_balance_nanos=available_balance_nanos,
            balance_nanos=balance_nanos,
            events_digest_hex=events_digest_hex,
            keys_digest_hex=keys_digest_hex,
            reserved_balance_nanos=reserved_balance_nanos,
            avatar_seed=avatar_seed,
            display_name=display_name,
            positions=positions,
        )


        account_response.additional_properties = d
        return account_response

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
