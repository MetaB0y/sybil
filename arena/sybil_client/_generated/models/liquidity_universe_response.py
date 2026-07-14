from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.actor_role import ActorRole
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="LiquidityUniverseResponse")



@_attrs_define
class LiquidityUniverseResponse:
    """ 
        Attributes:
            activated_at_height (int):
            actor_ready (bool):
            committed_market_ids (list[int]): Raw committed allow-list before lifecycle filtering. Controllers use
                this to distinguish a newly created market from a resolved old member.
            generation (int):
            market_ids (list[int]):
            policy_digest_hex (str):
            account_id (int | None | Unset):
            actor_role (ActorRole | None | Unset):
            principal_id (None | str | Unset): Present only on the actor-authenticated view. Lets a daemon fail closed
                if its local account configuration does not match its bound credential.
     """

    activated_at_height: int
    actor_ready: bool
    committed_market_ids: list[int]
    generation: int
    market_ids: list[int]
    policy_digest_hex: str
    account_id: int | None | Unset = UNSET
    actor_role: ActorRole | None | Unset = UNSET
    principal_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        activated_at_height = self.activated_at_height

        actor_ready = self.actor_ready

        committed_market_ids = self.committed_market_ids



        generation = self.generation

        market_ids = self.market_ids



        policy_digest_hex = self.policy_digest_hex

        account_id: int | None | Unset
        if isinstance(self.account_id, Unset):
            account_id = UNSET
        else:
            account_id = self.account_id

        actor_role: None | str | Unset
        if isinstance(self.actor_role, Unset):
            actor_role = UNSET
        elif isinstance(self.actor_role, ActorRole):
            actor_role = self.actor_role.value
        else:
            actor_role = self.actor_role

        principal_id: None | str | Unset
        if isinstance(self.principal_id, Unset):
            principal_id = UNSET
        else:
            principal_id = self.principal_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "activated_at_height": activated_at_height,
            "actor_ready": actor_ready,
            "committed_market_ids": committed_market_ids,
            "generation": generation,
            "market_ids": market_ids,
            "policy_digest_hex": policy_digest_hex,
        })
        if account_id is not UNSET:
            field_dict["account_id"] = account_id
        if actor_role is not UNSET:
            field_dict["actor_role"] = actor_role
        if principal_id is not UNSET:
            field_dict["principal_id"] = principal_id

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        activated_at_height = d.pop("activated_at_height")

        actor_ready = d.pop("actor_ready")

        committed_market_ids = cast(list[int], d.pop("committed_market_ids"))


        generation = d.pop("generation")

        market_ids = cast(list[int], d.pop("market_ids"))


        policy_digest_hex = d.pop("policy_digest_hex")

        def _parse_account_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        account_id = _parse_account_id(d.pop("account_id", UNSET))


        def _parse_actor_role(data: object) -> ActorRole | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                actor_role_type_1 = ActorRole(data)



                return actor_role_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ActorRole | None | Unset, data)

        actor_role = _parse_actor_role(d.pop("actor_role", UNSET))


        def _parse_principal_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        principal_id = _parse_principal_id(d.pop("principal_id", UNSET))


        liquidity_universe_response = cls(
            activated_at_height=activated_at_height,
            actor_ready=actor_ready,
            committed_market_ids=committed_market_ids,
            generation=generation,
            market_ids=market_ids,
            policy_digest_hex=policy_digest_hex,
            account_id=account_id,
            actor_role=actor_role,
            principal_id=principal_id,
        )


        liquidity_universe_response.additional_properties = d
        return liquidity_universe_response

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
