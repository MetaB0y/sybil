from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.history_event_response import HistoryEventResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    id: int,
    *,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["before"] = before

    params["category"] = category


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/accounts/{id}/events".format(id=quote(str(id), safe=""),),
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> list[HistoryEventResponse] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in (_response_200):
            response_200_item = HistoryEventResponse.from_dict(response_200_item_data)



            response_200.append(response_200_item)

        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[list[HistoryEventResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> Response[list[HistoryEventResponse]]:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[HistoryEventResponse]]
     """


    kwargs = _get_kwargs(
        id=id,
limit=limit,
before=before,
category=category,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> list[HistoryEventResponse] | None:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[HistoryEventResponse]
     """


    return sync_detailed(
        id=id,
client=client,
limit=limit,
before=before,
category=category,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> Response[list[HistoryEventResponse]]:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[HistoryEventResponse]]
     """


    kwargs = _get_kwargs(
        id=id,
limit=limit,
before=before,
category=category,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> list[HistoryEventResponse] | None:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[HistoryEventResponse]
     """


    return (await asyncio_detailed(
        id=id,
client=client,
limit=limit,
before=before,
category=category,

    )).parsed
